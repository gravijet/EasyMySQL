"""Windows-Clientvergleich mit isolierten Profilen; benoetigt psutil und Playwright."""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
from importlib.metadata import version
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import time
import urllib.request

import psutil
from playwright.sync_api import sync_playwright

parser = argparse.ArgumentParser()
parser.add_argument('--bench-root', type=Path, required=True)
parser.add_argument('--output', type=Path, default=Path(__file__).parent/'public/benchmarks.json')
parser.add_argument('--runs', type=int, default=5)
parser.add_argument('--only', choices=['easy','workbench','xampp'])
args = parser.parse_args()
basis = args.bench_root.resolve()
programme = {
    'easy': basis/'easymysql/EasyMySQL.exe',
    'workbench': basis/'workbench/MySQL/MySQL Workbench/MySQL Workbench.exe',
    'apache': basis/'xampp/xampp/apache/bin/httpd.exe',
}

def prozessbaum(wurzeln):
    gefunden = {}
    for pid in wurzeln:
        try:
            prozess = psutil.Process(pid)
            for p in [prozess, *prozess.children(recursive=True)]:
                gefunden[p.pid] = p
        except psutil.NoSuchProcess:
            pass
    return list(gefunden.values())

def stoppen(pid):
    for p in reversed(prozessbaum([pid])):
        try:
            p.kill()
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            pass
    time.sleep(.3)

def stichprobe(wurzeln):
    privat = zugesichert = 0
    cpu = {}
    namen = []
    for p in prozessbaum(wurzeln):
        try:
            privat += p.memory_full_info().uss
            zugesichert += p.memory_info().private
            zeiten = p.cpu_times()
            cpu[p.pid] = zeiten.user + zeiten.system
            namen.append(p.name())
        except psutil.NoSuchProcess:
            pass
    if not namen:
        raise RuntimeError('Kein Testprozess mehr aktiv')
    return {'private_working_set_mib':privat/1048576, 'private_commit_mib':zugesichert/1048576, 'cpu':cpu, 'processes':namen}

def leerlauf(wurzeln):
    time.sleep(8)
    start = time.perf_counter()
    vorher = stichprobe(wurzeln)
    cpuzeit = 0
    werte = []
    for _ in range(5):
        time.sleep(2)
        aktuell = stichprobe(wurzeln)
        cpuzeit += sum(max(0, t-vorher['cpu'].get(pid,0)) for pid,t in aktuell['cpu'].items())
        vorher = aktuell
        werte.append({k:v for k,v in aktuell.items() if k!='cpu'})
    dauer = time.perf_counter()-start
    return {
        'private_working_set_mib':statistics.median(w['private_working_set_mib'] for w in werte),
        'private_commit_mib':statistics.median(w['private_commit_mib'] for w in werte),
        'idle_cpu_percent_total':cpuzeit/dauer/(psutil.cpu_count() or 1)*100,
        'sample_seconds':dauer,
        'samples':werte,
    }

def starten(pfad, parameter, umgebung=None):
    info = subprocess.STARTUPINFO()
    info.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    info.wShowWindow = 0
    return subprocess.Popen([str(pfad),*parameter],env=umgebung,startupinfo=info,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)

def easy_profil():
    profil = basis/'profile-easy'
    for teil in ['program','local/EasyMySQL','roaming','home']:
        (profil/teil).mkdir(parents=True,exist_ok=True)
    (profil/'local/EasyMySQL/einstellungen.txt').write_text('language=en\nserver_autostart=0\nauto_install=0\nrestore_tabs=0\nbackground=0\nport=13307\napp_update_hours=720\n',encoding='utf-8')
    env = dict(os.environ)
    env.update(ProgramData=str(profil/'program'),LOCALAPPDATA=str(profil/'local'),APPDATA=str(profil/'roaming'),USERPROFILE=str(profil/'home'))
    return env

def cdp_bereit(port):
    ende = time.monotonic()+35
    while time.monotonic()<ende:
        try:
            with urllib.request.urlopen(f'http://127.0.0.1:{port}/json/version',timeout=.3) as antwort:
                if antwort.status==200:
                    return
        except Exception:
            time.sleep(.05)
    raise RuntimeError('Workbench-Debug-Endpunkt startet nicht')

eingabe_bereit = ctypes.windll.user32.WaitForInputIdle
eingabe_bereit.argtypes = [wintypes.HANDLE,wintypes.DWORD]
eingabe_bereit.restype = wintypes.DWORD

def native_messung(art, playwright, warmup=False):
    beginn = time.perf_counter()
    parameter = [] if art=='easy' else ['--remote-debugging-port=9228',f'--user-data-dir={basis/"profile-workbench"}']
    prozess = starten(programme[art],parameter,easy_profil() if art=='easy' else None)
    verbindung = None
    try:
        if art=='easy':
            if eingabe_bereit(wintypes.HANDLE(int(prozess._handle)),30000)!=0:
                raise RuntimeError('EasyMySQL erreicht keine bereite Eingabeschleife')
            marker = 'Windows WaitForInputIdle; GUI message loop ready, not a first-paint measurement'
        else:
            cdp_bereit(9228)
            verbindung = playwright.chromium.connect_over_cdp('http://127.0.0.1:9228')
            seite = verbindung.contexts[0].pages[0]
            seite.get_by_text('Welcome to MySQL Workbench',exact=True).wait_for()
            seite.evaluate('document.fonts.ready')
            marker = 'Workbench welcome page visible and fonts ready via CDP'
        dauer = (time.perf_counter()-beginn)*1000
        if warmup:
            time.sleep(2)
            return None
        werte = leerlauf([prozess.pid])
        werte.update(startup_ms=dauer,readiness_marker=marker)
        return werte
    finally:
        if verbindung:
            verbindung.close()
        stoppen(prozess.pid)

def xampp_messung(playwright,warmup=False):
    # Apache/PHP laeuft bereits; der Datenbankserver wird nicht zum Client gezaehlt.
    apache = [p.pid for p in psutil.process_iter(['exe']) if p.info['exe'] and Path(p.info['exe']).resolve()==programme['apache']]
    if not apache:
        raise RuntimeError('Isolierter Apache muss vor dem Vergleich laufen')
    vorhandene = {p.pid for p in psutil.Process().children(recursive=True)}
    beginn = time.perf_counter()
    context = playwright.chromium.launch_persistent_context(str(basis/'profile-phpmyadmin-python'),channel='chrome',headless=True,viewport={'width':1200,'height':760},locale='en-US')
    try:
        seite = context.pages[0]
        seite.goto('http://127.0.0.1:18080/phpmyadmin/',wait_until='load')
        assert '13307' in seite.locator('body').inner_text(), 'Test muss den isolierten Datenbankserver zeigen'
        assert 'phpMyAdmin' in seite.title()
        seite.evaluate('document.fonts.ready')
        dauer = (time.perf_counter()-beginn)*1000
        neu = [p for p in psutil.Process().children(recursive=True) if p.pid not in vorhandene and p.name().lower()=='chrome.exe' and not any(a.startswith('--type=') for a in p.cmdline())]
        if len(neu)!=1:
            raise RuntimeError(f'Browser-Prozess nicht eindeutig: {len(neu)}')
        if warmup:
            time.sleep(2)
            return None
        werte = leerlauf([neu[0].pid,*apache])
        werte.update(startup_ms=dauer,readiness_marker='Dedicated headless Chrome: phpMyAdmin loaded, port 13307 visible and fonts ready; Apache/PHP already running')
        return werte
    finally:
        context.close()

ergebnisse = {}
with sync_playwright() as playwright:
    for art in ([args.only] if args.only else ['easy','workbench','xampp']):
        messen = (lambda warmup=False: xampp_messung(playwright,warmup)) if art=='xampp' else (lambda warmup=False: native_messung(art,playwright,warmup))
        print(f'{art}: Aufwaermen',flush=True)
        messen(True)
        laeufe = []
        for i in range(args.runs):
            lauf = messen()
            laeufe.append(lauf)
            print(f'{art} {i+1}/{args.runs}: {lauf["private_working_set_mib"]:.1f} MiB RAM; {lauf["startup_ms"]:.0f} ms Start; {lauf["idle_cpu_percent_total"]:.3f}% CPU',flush=True)
        ergebnisse[art] = {
            'runs':laeufe,
            'median':{feld:statistics.median(l[feld] for l in laeufe) for feld in ['private_working_set_mib','private_commit_mib','startup_ms','idle_cpu_percent_total']},
        }

daten = {
    'schema':'easymysql-client-runtime-v1',
    'measured_at':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),
    'platform':platform.platform(),
    'cpu':'AMD Ryzen 7 8700G',
    'logical_processors':psutil.cpu_count(),
    'physical_ram_gib':round(psutil.virtual_memory().total/1073741824,1),
    'runs_per_app':args.runs,
    'settle_seconds':8,
    'idle_measurement_seconds':10,
    'versions':{'easy':'7.1.0','workbench':'26.10.0','xampp':'8.2.12 Portable','phpmyadmin':'5.2.1','apache':'2.4.58','php':'8.2.12','chrome':'156.0.8078.81','shared_mariadb':'10.4.32'},
    'scope':'Client start view, no active query, isolated profiles. Private resident working set (USS), all client subprocesses; XAMPP includes Apache/PHP and dedicated headless Chrome. Shared MariaDB excluded from every client. GPU memory is excluded. Start markers differ by application; these are instrumented warm-start readiness measurements, not identical first-paint measurements. Disk/OS caches remain warm; one unrecorded warmup per app.',
    'results':ergebnisse,
    'sources':{'easy':'https://github.com/gravijet/EasyMySQL/releases/tag/v7.1.0','workbench':'https://dev.mysql.com/downloads/workbench/','xampp':'https://sourceforge.net/projects/xampp/files/XAMPP%20Windows/8.2.12/'},
    'binary_sha256':{art:hashlib.sha256(programme[art].read_bytes()).hexdigest() for art in ['easy','workbench','apache']},
    'measurement_tools':{'python':platform.python_version(),'psutil':psutil.__version__,'playwright':version('playwright')},
    'source_notes':'Workbench 26 is Electron-based; legacy Workbench 8 modeling claims are not applied to this tested version. XAMPP is the official 8.2.12 portable package; newer 8.5 installer was not used.',
}
if args.only:
    args.output = args.output.with_name(f'runtime-{args.only}.json')
args.output.write_text(json.dumps(daten,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
print(f'Gespeichert: {args.output}',flush=True)
