"""Regenerates every app icon from the master artwork.

Run after changing `media/ninja2.png`:

    python resources/scripts/generate-icons.py

Requires Pillow (`pip install pillow`).
"""

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]

# Dark artwork, used for app/installer icons and on light backgrounds.
MASTER = ROOT / "media" / "ninja2.png"

# Light artwork, used wherever the icon sits on a dark surface. The system
# tray is the case that matters: a dark icon is close to invisible on a
# dark taskbar.
MASTER_LIGHT = ROOT / "media" / "ninja3.png"

# Multi-resolution ICO. Windows picks the closest size for each context
# (taskbar, alt-tab, explorer, installer), so all of these matter.
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]

# macOS iconset sizes, used for the ICNS.
ICNS_SIZES = [16, 32, 64, 128, 256, 512, 1024]


def square(image: Image.Image) -> Image.Image:
    """Pads the artwork to a square canvas.

    The master isn't square, and stretching it would distort the logo.
    Padding keeps the aspect ratio and centres it.
    """
    side = max(image.size)
    canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    canvas.paste(
        image,
        ((side - image.width) // 2, (side - image.height) // 2),
    )
    return canvas


def resized(master: Image.Image, size: int) -> Image.Image:
    return master.resize((size, size), Image.LANCZOS)


def write_png(master: Image.Image, path: Path, size: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    resized(master, size).save(path, "PNG")
    print(f"  {path.relative_to(ROOT)} ({size}x{size})")


def main() -> None:
    if not MASTER.exists():
        raise SystemExit(f"Master artwork not found: {MASTER}")

    master = square(Image.open(MASTER).convert("RGBA"))
    print(f"Master: {MASTER.relative_to(ROOT)} -> {master.size[0]}px square")

    # --- Window manager ---
    print("Window manager:")
    write_png(master, ROOT / "resources/assets/icon.png", 512)

    ico_path = ROOT / "resources/assets/icon.ico"
    resized(master, 256).save(
        ico_path, "ICO", sizes=[(s, s) for s in ICO_SIZES]
    )
    print(f"  {ico_path.relative_to(ROOT)} ({', '.join(map(str, ICO_SIZES))})")

    # --- Bar (Tauri) ---
    print("Bar:")
    icons = ROOT / "bar/packages/desktop/resources/icons"
    write_png(master, icons / "32x32.png", 32)
    write_png(master, icons / "128x128.png", 128)
    write_png(master, icons / "128x128@2x.png", 256)
    write_png(master, icons / "icon.png", 512)

    bar_ico = icons / "icon.ico"
    resized(master, 256).save(
        bar_ico, "ICO", sizes=[(s, s) for s in ICO_SIZES]
    )
    print(f"  {bar_ico.relative_to(ROOT)} ({', '.join(map(str, ICO_SIZES))})")

    icns_path = icons / "icon.icns"
    resized(master, 1024).save(
        icns_path, "ICNS", sizes=[(s, s) for s in ICNS_SIZES]
    )
    print(f"  {icns_path.relative_to(ROOT)} (icns)")

    # --- Settings UI ---
    print("Settings UI:")
    write_png(
        master,
        ROOT / "bar/packages/settings-ui/resources/logo-128x128.png",
        128,
    )

    # --- Light variant, for dark backgrounds ---
    if not MASTER_LIGHT.exists():
        print(f"Skipping light variant: {MASTER_LIGHT} not found")
        return

    light = square(Image.open(MASTER_LIGHT).convert("RGBA"))
    print("Light variant:")
    write_png(light, ROOT / "resources/assets/icon-light.png", 512)

    # Shown in the bar itself, which draws light-on-dark.
    write_png(light, ROOT / "bar/resources/starter/logo.png", 64)
    write_png(
        light,
        ROOT / "bar/packages/settings-ui/resources/logo-128x128-light.png",
        128,
    )


if __name__ == "__main__":
    main()
