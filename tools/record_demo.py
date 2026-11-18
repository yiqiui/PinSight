"""Record the PinSight demo GIF end-to-end.

Drives a real running PinSight through the storyboard while a background
thread continuously captures a fixed screen region, then assembles a GIF.

Privacy: only a fixed region over our own demo windows is captured; a test
frame is saved first for inspection before the real run.
"""
import ctypes, ctypes.wintypes as w, os, subprocess, sys, threading, time, shutil

user32 = ctypes.windll.user32
SM_X, SM_Y = 0, 1
CWD = r"D:\workspace\code\ws\PinSight"
EXE = os.path.join(CWD, "target", "release", "pinsight.exe")
OUT_DIR = os.path.join(CWD, ".shots")
CFG_DIR = os.path.join(OUT_DIR, "demo_cfg")
PAGE = "file:///" + os.path.join(CWD, "docs", "demo_page.html").replace("\\", "/")
# capture region (physical px), inside: demo browser window + overlay + pins
REGION = (90, 60, 930, 700)  # left, top, right, bottom
FPS = 15

SW_RESTORE = 9

def find_windows(title_sub, want_pid=None):
    out = []
    @ctypes.WINFUNCTYPE(ctypes.c_bool, w.HWND, w.LPARAM)
    def cb(hwnd, lparam):
        if user32.IsWindowVisible(hwnd):
            buf = ctypes.create_unicode_buffer(256)
            user32.GetWindowTextW(hwnd, buf, 256)
            if title_sub.lower() in buf.value.lower():
                pid = w.DWORD()
                user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
                if want_pid is None or pid.value == want_pid:
                    out.append((hwnd, buf.value, pid.value))
        return True
    user32.EnumWindows(cb, 0)
    return out

def rect_of(hwnd):
    r = w.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    return (r.left, r.top, r.right, r.bottom)

def move_win(hwnd, x, y, ww, hh):
    user32.ShowWindow(hwnd, SW_RESTORE)
    time.sleep(0.2)
    user32.MoveWindow(hwnd, x, y, ww, hh, True)
    time.sleep(0.3)

VK = {"ctrl": 0x11, "shift": 0x10, "alt": 0x12, "esc": 0x1B, "enter": 0x0D,
      "p": 0x50, "a": 0x41, "r": 0x52, "n": 0x4E, "t": 0x54, "e": 0x45,
      "d": 0x44, "h": 0x48, "m": 0x4D, "l": 0x4C, "k": 0x4B, "v": 0x56,
      "s": 0x53, "z": 0x5A, "y": 0x59, "f": 0x46, "g": 0x47,
      "1": 0x31, "2": 0x32, "space": 0x20}

def key(*names):
    for n in names:
        user32.keybd_event(VK[n], 0, 0, 0)
    time.sleep(0.06)
    for n in reversed(names):
        user32.keybd_event(VK[n], 0, 0x0002, 0, 0)

def move_to(x, y, steps=14):
    p = w.POINT()
    user32.GetCursorPos(ctypes.byref(p))
    for i in range(1, steps + 1):
        nx = p.x + (x - p.x) * i / steps
        ny = p.y + (y - p.y) * i / steps
        user32.SetCursorPos(int(nx), int(ny))
        time.sleep(0.02)

def smooth_drag(x1, y1, x2, y2, steps=26):
    move_to(x1, y1, steps=10)
    time.sleep(0.15)
    user32.mouse_event(0x0002, 0, 0, 0, 0)  # down
    time.sleep(0.12)
    for i in range(1, steps + 1):
        nx = x1 + (x2 - x1) * i / steps
        ny = y1 + (y2 - y1) * i / steps
        user32.SetCursorPos(int(nx), int(ny))
        time.sleep(0.035)
    time.sleep(0.12)
    user32.mouse_event(0x0004, 0, 0, 0, 0)  # up
    time.sleep(0.2)

def click(x, y):
    move_to(x, y, steps=8)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.07)
    user32.mouse_event(0x0004, 0, 0, 0, 0)

class Recorder:
    def __init__(self, region):
        import mss
        self.region = region
        self.frames = []          # (t, png_bytes)
        self.stop = threading.Event()
        self._mss = mss.mss()
        self.mon = {"left": region[0], "top": region[1],
                    "width": region[2] - region[0], "height": region[3] - region[1]}
        self.thr = threading.Thread(target=self._loop, daemon=True)

    def _loop(self):
        import mss
        while not self.stop.is_set():
            raw = self._mss.grab(self.mon)
            self.frames.append((time.monotonic(), raw))
            time.sleep(1.0 / FPS)

    def start(self): self.thr.start()
    def stop_rec(self):
        self.stop.set(); self.thr.join(timeout=2)

def build_gif(frames, out_path, region, target_w=800):
    from PIL import Image, ImageChops, ImageStat
    imgs = []
    for t, raw in frames:
        im = Image.frombytes("RGB", raw.size, raw.bgra, "raw", "BGRX")
        s = target_w / im.width
        imgs.append(im.resize((target_w, int(im.height * s)), Image.LANCZOS))
    # drop near-identical frames so static holds don't bloat the GIF,
    # but never skip more than ~0.4 s of content
    kept = [imgs[0]]
    drop_run = 0
    for im in imgs[1:]:
        diff = ImageStat.Stat(ImageChops.difference(kept[-1], im)).mean
        if sum(diff) < 3.0 and drop_run < 6:
            drop_run += 1
            continue
        drop_run = 0
        kept.append(im)
    kept[0].save(out_path, save_all=True, append_images=kept[1:],
                 duration=int(1000 / FPS), loop=0, optimize=True)
    return len(kept)

def close_demo_window(hwnd):
    WM_CLOSE = 0x0010
    user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)

def main():
    test_only = "--test-frame" in sys.argv

    # clean environment: only kill our own app; NEVER kill the user's browser
    subprocess.run(["taskkill", "/f", "/im", "pinsight.exe"], capture_output=True)
    time.sleep(1)
    shutil.rmtree(CFG_DIR, ignore_errors=True)
    os.makedirs(CFG_DIR, exist_ok=True)

    rec = Recorder(REGION)

    if test_only:
        # open demo page + app, park them, grab one frame for inspection
        subprocess.Popen(["cmd", "/c", f"start msedge --new-window --app={PAGE}"], shell=False)
        time.sleep(4)
        edge = find_windows("PinSight Demo Page")
        print("edge windows:", edge)
        if edge:
            # fully cover the capture region so no user windows leak in
            move_win(edge[0][0], 80, 50, 880, 700)
        env = dict(os.environ, PINSIGHT_CONFIG_DIR=CFG_DIR)
        proc = subprocess.Popen([EXE], cwd=CWD, env=env,
                                creationflags=subprocess.CREATE_NEW_PROCESS_GROUP)
        time.sleep(6)
        app = find_windows("PinSight", want_pid=proc.pid)
        print("app windows:", app)
        if app:
            move_win(app[0][0], 960, 80, 800, 560)
        rec.start(); time.sleep(2); rec.stop_rec()
        build_gif(rec.frames, os.path.join(OUT_DIR, "test_frame.png"), REGION)
        proc.kill()
        if edge:
            close_demo_window(edge[0][0])
        print("test frame written")
        return

    # ---------------- real run ----------------
    import mss  # noqa
    log = open(os.path.join(OUT_DIR, "demo.log"), "w", encoding="utf-8")

    def say(msg, t="0.5"):
        print(msg, flush=True); log.write(msg + "\n"); log.flush()
        time.sleep(float(t))

    # 1. demo page in app-mode edge, fully covering the capture region
    say("open demo page", "0")
    subprocess.Popen(["cmd", "/c", f"start msedge --new-window --app={PAGE}"], shell=False)
    time.sleep(4)
    edge = find_windows("PinSight Demo Page")
    if edge:
        move_win(edge[0][0], 80, 50, 880, 700)
    else:
        say("!! demo page window not found", "0")

    # 2. fresh-config app instance (stderr logged for diagnosis)
    say("launch pinsight", "0")
    env = dict(os.environ, PINSIGHT_CONFIG_DIR=CFG_DIR, RUST_LOG="debug")
    applog = open(os.path.join(OUT_DIR, "demo_app.log"), "w", encoding="utf-8")
    proc = subprocess.Popen([EXE], cwd=CWD, env=env, stdout=applog, stderr=subprocess.STDOUT,
                            creationflags=subprocess.CREATE_NEW_PROCESS_GROUP)
    time.sleep(6)
    app = find_windows("PinSight", want_pid=proc.pid)
    if app:
        move_win(app[0][0], 960, 80, 800, 560)
    time.sleep(1)

    rec.start()
    t0 = time.monotonic()

    # Scene 1: region capture + annotate
    key("ctrl", "shift", "a"); say("hotkey capture", "1.2")
    smooth_drag(200, 150, 700, 480); say("drag selection", "0.8")

    key("r"); smooth_drag(240, 220, 420, 300); say("rect annotate", "0.5")
    key("a"); smooth_drag(450, 330, 580, 260); say("arrow annotate", "0.5")
    key("n"); click(300, 370); click(470, 400); say("numbered steps", "0.7")

    # Scene 2: pin it
    key("p"); say("pin!", "1.4")
    # wheel zoom over pin
    move_to(430, 300)
    user32.mouse_event(0x0800, 0, 0, 120, 0); time.sleep(0.18)
    user32.mouse_event(0x0800, 0, 0, 120, 0); time.sleep(0.18)
    say("pin zoom", "0.3")
    # drag pin toward the right (inside the capture region)
    smooth_drag(430, 300, 620, 320, steps=30); say("pin drag", "0.9")

    # Scene 3: long capture over the page
    key("ctrl", "shift", "l"); say("long capture hotkey", "1.2")
    smooth_drag(200, 150, 820, 560); say("long selection", "0.8")
    key("enter"); say("long stitching...", "1.0")

    # wait until the stitched file actually lands in the save dir (max 15 s)
    save_dir = os.path.expandvars(r"%USERPROFILE%\Pictures\PinSight")
    t_start = time.time()
    saved_path = None
    while time.time() - t_start < 15:
        try:
            cand = [os.path.join(save_dir, f) for f in os.listdir(save_dir)
                    if f.endswith(".png")
                    and os.path.getmtime(os.path.join(save_dir, f)) > t_start - 2]
            if cand:
                newest = max(cand, key=os.path.getmtime)
                s1 = os.path.getsize(newest)
                time.sleep(0.8)
                if os.path.getsize(newest) == s1:
                    saved_path = newest
                    break
        except OSError:
            pass
        time.sleep(0.4)
    say(f"stitched: {saved_path}" if saved_path else "!! no long capture file", "1.0")

    # bring the main window into the capture region so the toast + picker
    # dialog (rendered inside it) are visible in the GIF
    if app:
        move_win(app[0][0], 110, 300, 620, 370)
    say("toast: long saved", "2.6")

    # Scene 4: colour picker over the page hero (visible above the main window)
    key("ctrl", "shift", "k"); say("picker hotkey", "0.6")
    move_to(420, 190, steps=16)
    time.sleep(0.7)
    click(420, 190); say("picked colour", "1.0")

    # Scene 5: outro — hold a beat then stop
    say("done", "1.5")
    rec.stop_rec()
    dur = time.monotonic() - t0
    proc.kill()
    if edge:
        close_demo_window(edge[0][0])

    out = os.path.join(OUT_DIR, "demo.gif")
    n = build_gif(rec.frames, out, REGION)
    print(f"frames={n} seconds={dur:.1f} -> {out}", flush=True)

if __name__ == "__main__":
    main()
