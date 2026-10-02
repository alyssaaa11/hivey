#!/usr/bin/env python3
"""hiver pets: choose, build, start, switch and turn off the desktop pet (macOS).

usage: pet.py [status]            the chosen pet, running or not, and the pets to choose from
       pet.py list [--json]       the pets
       pet.py use <id>            build (when needed), install and start it; the other pets
                                  are quit
       pet.py show                start the chosen pet if it isn't running (hiver runs it when
                                  a hiver window opens; the pet quits itself when the last
                                  hiver window closes)
       pet.py off                 quit the pet and stop opening it at login
       pet.py choose              pick one interactively (install.sh, the ⌥P popup)
       pet.py refresh             rebuild and restart the chosen pet if its source changed
                                  (hiver update runs it)

Run through `hiver pet …` (which sets HIVER_BIN). Each pet lives in pets/<id>/ (main.swift,
build.sh) and shares pets/shared/HiverWatch.swift; it is built into its folder and installed
to ~/Applications. The choice is kept in ~/.hiver/pet.json ({"pet": "<id>"} or {"pet": null}).
"""
import json
import os
import platform
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
PETS = {
    "hiver-h": {"name": "Hiver H", "app": "Hiver H.app", "bundle": "com.hiver.h",
                "about": "a living 3D H: its sections slide apart to hand out work"},
    "hiver-dot": {"name": "Hiver", "app": "Hiver.app", "bundle": "com.hiver.pet",
                  "about": "a graphite dot; its mint agents ride a signal wave around it"},
    "hiver-prompt": {"name": "Hiver Prompt", "app": "Hiver Prompt.app", "bundle": "com.hiver.prompt",
                     "about": "a >_ terminal sphere leading three agent spheres"},
}
ORDER = ["hiver-h", "hiver-dot", "hiver-prompt"]


def home():
    return Path(os.environ.get("HOME", str(Path.home())))


def choice_path():
    return home() / ".hiver" / "pet.json"


def apps_dir():
    return Path(os.environ.get("HIVER_PETS_APPS", str(home() / "Applications")))


def launch_agent(pet):
    return home() / "Library" / "LaunchAgents" / f"{PETS[pet]['bundle']}.plist"


def installed_app(pet):
    return apps_dir() / PETS[pet]["app"]


def chosen():
    try:
        return json.loads(choice_path().read_text()).get("pet")
    except (OSError, json.JSONDecodeError, AttributeError):
        return None


def save_choice(pet):
    choice_path().parent.mkdir(parents=True, exist_ok=True)
    choice_path().write_text(json.dumps({"pet": pet}) + "\n")


def pids(pet):
    """Processes whose executable is this pet's app. `pgrep -a` also searches our ancestors: a
    switch from a pet's menu runs as that pet's child, and macOS pgrep skips ancestors by
    default. Matching the executable (`ps -o comm=`), not the command line, keeps a shell whose
    command merely mentions the app from ever matching."""
    marker = f"{PETS[pet]['app']}/Contents/MacOS/"
    found = subprocess.run(["pgrep", "-a", "-f", marker], capture_output=True, text=True)
    result = []
    for pid in found.stdout.split():
        comm = subprocess.run(["ps", "-o", "comm=", "-p", pid], capture_output=True, text=True)
        if marker in comm.stdout:
            result.append(int(pid))
    return result


def running(pet):
    return bool(pids(pet))


def sources(pet):
    return [HERE / pet / "main.swift", HERE / pet / "build.sh", HERE / "shared" / "HiverWatch.swift"]


def outdated(pet):
    """True when the installed app is missing or older than its source."""
    app = installed_app(pet) / "Contents" / "MacOS"
    if not app.is_dir():
        return True
    built = min((f.stat().st_mtime for f in app.iterdir()), default=0)
    return any(src.stat().st_mtime > built for src in sources(pet) if src.exists())


def check_mac():
    if platform.system() != "Darwin":
        raise SystemExit("hiver pets are macOS apps; this is " + platform.system())
    if not shutil.which("swiftc"):
        raise SystemExit("building a pet needs Swift: install Apple's command line tools "
                         "(xcode-select --install), then try again")


def build(pet):
    print(f"building {PETS[pet]['name']} (about a minute)…", flush=True)
    log = Path(os.environ.get("TMPDIR", "/tmp").rstrip("/")) / f"hiver-pet-{pet}.log"
    with open(log, "w") as out:
        done = subprocess.run(["bash", str(HERE / pet / "build.sh")], stdout=out, stderr=out)
    if done.returncode != 0:
        tail = log.read_text().strip().splitlines()[-8:]
        raise SystemExit("\n".join(tail) + f"\nbuilding {PETS[pet]['name']} failed (log: {log})")
    apps_dir().mkdir(parents=True, exist_ok=True)
    target = installed_app(pet)
    if target.exists():
        shutil.rmtree(target)
    # ditto keeps the ad-hoc signature intact
    subprocess.run(["ditto", str(HERE / pet / PETS[pet]["app"]), str(target)], check=True)


def quit_pet(pet):
    """Quit with a signal, not AppleScript: a switch started from a pet's own menu runs inside
    that app, and macOS may block (or silently ask about) one app scripting another."""
    for sig, tries in ((signal.SIGTERM, 15), (signal.SIGKILL, 10)):
        targets = pids(pet)
        if not targets:
            return
        for pid in targets:
            try:
                os.kill(pid, sig)
            except ProcessLookupError:
                pass
        for _ in range(tries):
            if not running(pet):
                return
            time.sleep(0.2)
    print(f"could not quit {PETS[pet]['name']}", file=sys.stderr)


def set_login(pet, on):
    plist = launch_agent(pet)
    if not on:
        plist.unlink(missing_ok=True)
        return
    plist.parent.mkdir(parents=True, exist_ok=True)
    plist.write_text(f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{PETS[pet]['bundle']}</string>
    <key>ProgramArguments</key>
    <array><string>/usr/bin/open</string><string>-a</string><string>{installed_app(pet)}</string></array>
    <key>RunAtLoad</key><true/>
</dict>
</plist>
""")


def use(pet):
    if pet not in PETS:
        raise SystemExit(f"no pet {pet!r}: choose one of {', '.join(ORDER)} (or: hiver pet off)")
    check_mac()
    if outdated(pet):
        build(pet)
    for other in ORDER:
        # Pets come and go with hiver now: no login items (older versions added them)
        set_login(other, False)
        if other != pet:
            quit_pet(other)
    quit_pet(pet)   # a rebuilt pet restarts with the new code
    subprocess.run(["open", "-a", str(installed_app(pet))], check=True)
    save_choice(pet)
    print(f"{PETS[pet]['name']} is your pet: it shows while hiver is open (change it: hiver pet)")


def show():
    """Start the chosen pet if it isn't running; quiet when there is nothing to do."""
    pet = chosen()
    if pet not in PETS or platform.system() != "Darwin" or running(pet):
        return
    if outdated(pet):
        if not shutil.which("swiftc"):
            return
        build(pet)
    subprocess.run(["open", "-g", "-a", str(installed_app(pet))], check=False)


def off():
    for pet in ORDER:
        quit_pet(pet)
        set_login(pet, False)
    save_choice(None)
    print("pet turned off (back any time: hiver pet use <id>, or hiver pet choose)")


def status():
    current = chosen()
    if current in PETS:
        state = "running" if running(current) else "shows when a hiver window is open"
        print(f"pet: {PETS[current]['name']} ({current}), {state}")
    else:
        print("pet: none")
    print_list(current)
    print("\nhiver pet use <id> · hiver pet off · hiver pet choose")


def print_list(current):
    for pet in ORDER:
        mark = "●" if pet == current else " "
        print(f"  {mark} {pet:<13} {PETS[pet]['name']:<13} {PETS[pet]['about']}")


def choose():
    current = chosen()
    print("Choose your hiver pet (it lives on your desktop and acts out what your agents do):\n")
    for n, pet in enumerate(ORDER, 1):
        mark = " (current)" if pet == current else ""
        print(f"  {n}. {PETS[pet]['name']:<13} {PETS[pet]['about']}{mark}")
    print("  0. No pet")
    keep = "keep the current one" if current in PETS else "no pet"
    try:
        answer = input(f"\nNumber (Enter: {keep}): ").strip()
    except (EOFError, KeyboardInterrupt):
        print()
        return 1
    if not answer:
        print("unchanged")
        return 0
    if answer == "0":
        off()
        return 0
    if answer.isdigit() and 1 <= int(answer) <= len(ORDER):
        use(ORDER[int(answer) - 1])
        return 0
    print(f"{answer!r} is not one of the numbers; nothing changed")
    return 1


def refresh():
    pet = chosen()
    if pet not in PETS or platform.system() != "Darwin" or not outdated(pet):
        return
    if not shutil.which("swiftc"):
        print("pet: its source changed, but Swift is missing to rebuild it")
        return
    use(pet)


def main(argv):
    command = argv[0] if argv else "status"
    if command in ("-h", "--help", "help"):
        print(__doc__.split("\n\n")[1])
        return 0
    if command == "status":
        status()
    elif command == "list":
        if "--json" in argv:
            print(json.dumps({"current": chosen(), "pets": [
                {"id": pet, **{k: PETS[pet][k] for k in ("name", "about")}} for pet in ORDER]}))
        else:
            print_list(chosen())
    elif command == "use" and len(argv) == 2:
        use(argv[1])
    elif command == "off":
        off()
    elif command == "show":
        show()
    elif command == "choose":
        return choose()
    elif command == "refresh":
        refresh()
    else:
        print(__doc__.split("\n\n")[1], file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
