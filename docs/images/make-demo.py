"""Writes the demo files the README pictures show, and the scene list scripts\\shot.ps1 reads.

    python docs/images/make-demo.py <root> <scenes.json>

<root> should be a short path, such as a drive letter made with `subst X: <folder>`, because the
path bars show it. Everything here is made up: a small online shop's release 1.4 against 1.5.
"""
import json
import os
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

STAMP = 1_793_000_000  # one fixed modified time for files meant to look the same (Oct 2026)

CHECKOUT_14 = """import { Cart } from "./cart";
import { formatBaht } from "./money";

const TIMEOUT_MS = 15000;
const MESSAGES = {
  success: "ชำระเงินสำเร็จ ขอบคุณที่ใช้บริการ",
  failed: "ชำระเงินไม่สำเร็จ กรุณาลองใหม่",
  expired: "หมดเวลาชำระเงิน",
};

export function total(cart: Cart): number {
  let sum = 0;
  for (const item of cart.items) {
    sum += item.price * item.qty;
  }
  return sum;
}

export function applyCoupon(sum: number, code: string): number {
  if (code === "SAVE10") {
    return sum * 0.9;
  }
  return sum;
}

export async function checkout(cart: Cart, code: string) {
  const sum = applyCoupon(total(cart), code);
  const res = await pay(sum, { timeout: TIMEOUT_MS });
  if (!res.ok) {
    return { ok: false, message: MESSAGES.failed };
  }
  console.log("paid", formatBaht(sum));
  return { ok: true, message: MESSAGES.success };
}

function pay(amount: number, opts: { timeout: number }) {
  return fetch("/api/pay", {
    method: "POST",
    body: JSON.stringify({ amount }),
    signal: AbortSignal.timeout(opts.timeout),
  });
}
"""

CHECKOUT_15 = """import { Cart } from "./cart";
import { formatBaht } from "./money";

const TIMEOUT_MS = 30000;
const VAT_RATE = 0.07;
const MESSAGES = {
  success: "ชำระเงินเรียบร้อย ขอบคุณที่อุดหนุน",
  failed: "ชำระเงินไม่สำเร็จ กรุณาลองใหม่",
  expired: "หมดเวลาชำระเงิน",
  promptpay: "สแกน QR พร้อมเพย์เพื่อชำระเงิน",
};

export function total(cart: Cart): number {
  const sum = cart.items.reduce((s, item) => s + item.price * item.qty, 0);
  return sum * (1 + VAT_RATE);
}

export async function checkout(cart: Cart, code: string, method = "card") {
  const sum = applyCoupon(total(cart), code);
  const res = await pay(sum, method, { timeout: TIMEOUT_MS });
  if (!res.ok) {
    return { ok: false, message: MESSAGES.failed };
  }
  return { ok: true, message: MESSAGES.success };
}

export function applyCoupon(sum: number, code: string): number {
  if (code === "SAVE10") {
    return sum * 0.9;
  }
  return sum;
}

function pay(amount: number, method: string, opts: { timeout: number }) {
  return fetch("/api/pay", {
    method: "POST",
    body: JSON.stringify({ amount, method }),
    signal: AbortSignal.timeout(opts.timeout),
  });
}
"""

PRODUCTS_14 = """sku,name,price,stock
A100,กาแฟคั่วเข้ม 250 ก.,189,40
A101,ชาเขียวมัทฉะ,159,25
A102,แก้วเซรามิก,120,60
A103,ถุงผ้าลดโลกร้อน,59,100
A104,เมล็ดกาแฟอาราบิก้า 1 กก.,690,12
A105,ช้อนไม้,25,300
"""

PRODUCTS_15 = """sku,name,price,stock
A100,กาแฟคั่วเข้ม 250 ก.,189,32
A101,ชาเขียวมัทฉะ,169,25
A102,แก้วเซรามิก ลายช้าง,120,60
A103,ถุงผ้าลดโลกร้อน,59,100
A104,เมล็ดกาแฟอาราบิก้า 1 กก.,650,12
A105,ช้อนไม้,25,300
A106,ดริปเปอร์ V60,450,8
"""

MERGE_BASE = """app.name=Shop
app.timeout=15
ui.language=th
payment.methods=card
log.level=info
ui.banner=ลดราคา 30%
"""
MERGE_LEFT = """app.name=Shop
app.timeout=30
ui.language=th
payment.methods=card
log.level=info
ui.banner=ลดราคา 40%
"""
MERGE_RIGHT = """app.name=Shop
app.timeout=15
ui.language=th
payment.methods=card,promptpay
log.level=info
ui.banner=ลดราคาสูงสุด 50%
"""

README_DOC = "# Shop\n\nA small online shop.\n\n## Run\n\n    npm install\n    npm start\n"
DEPLOY = "#!/bin/bash\nset -euo pipefail\nnpm ci\nnpm run build\nrsync -a dist/ web:/srv/shop/\n"
TH_14 = '{\n  "checkout": "ชำระเงิน",\n  "cart": "ตะกร้าสินค้า",\n  "search": "ค้นหาสินค้า"\n}\n'
# 1.5 slipped a no-break space into one value: it looks the same, but a program reads it differently
TH_15 = '{\n  "checkout": "ชำระ\u00a0เงิน",\n  "cart": "ตะกร้าสินค้า",\n  "search": "ค้นหาสินค้า",\n  "promptpay": "พร้อมเพย์"\n}\n'
APP_14 = "server.port=8080\nsession.timeout=15\nfeature.promptpay=false\n"
APP_15 = "server.port=8080\nsession.timeout=30\nfeature.promptpay=true\n"
DB = "host: db.internal\nport: 5432\npool: 10\n"
CART = "export interface Cart {\n  items: { price: number; qty: number }[];\n}\n"
COUPON = "export const LEGACY_CODES = [\"OLD5\"];\n"
PROMPTPAY = "export function qr(amount: number) {\n  return `promptpay://${amount}`;\n}\n"


def font(size, bold=False):
    name = "seguisb.ttf" if bold else "segoeui.ttf"
    return ImageFont.truetype(str(Path("C:/Windows/Fonts") / name), size)


def banner(path, sale, colour):
    """A shop banner: the two releases differ in the sale figure and the badge colour."""
    w, h = 720, 360
    im = Image.new("RGB", (w, h), (250, 246, 238))
    d = ImageDraw.Draw(im)
    for y in range(h):
        t = y / h
        d.line([(0, y), (w, y)], fill=(round(250 - 30 * t), round(236 - 40 * t), round(214 - 60 * t)))
    d.rounded_rectangle([40, 40, 300, 320], radius=28, fill=(255, 255, 255))
    d.ellipse([95, 80, 245, 230], fill=(120, 72, 40))
    d.rectangle([150, 220, 190, 290], fill=(120, 72, 40))
    d.text((340, 70), "COFFEE", font=font(64, True), fill=(60, 40, 25))
    d.text((340, 150), f"SALE {sale}", font=font(56, True), fill=colour)
    d.rounded_rectangle([340, 240, 600, 300], radius=30, fill=colour)
    d.text((470, 270), "SHOP NOW", font=font(28, True), fill=(255, 255, 255), anchor="mm")
    im.save(path)


def firmware(path, variant):
    data = bytearray(b"DUETFW\x00\x01" + bytes(range(256)) * 6)
    if variant == 2:
        data[0x40:0x48] = b"\xde\xad\xbe\xef\x00\x00\x00\x02"
        data[0x200:0x204] = b"\x10\x20\x30\x40"
        data += b"\x00" * 16 + b"build 1.5"
    path.write_bytes(bytes(data))


def write(path, text, eol="\n", stamp=True):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.replace("\n", eol).encode("utf-8"))
    if stamp:
        os.utime(path, (STAMP, STAMP))


def main(root, scenes_path):
    root = Path(root)
    a, b = root / "shop-1.4", root / "shop-1.5"
    write(a / "src" / "checkout.ts", CHECKOUT_14)
    write(b / "src" / "checkout.ts", CHECKOUT_15)
    write(a / "src" / "cart.ts", CART)
    write(b / "src" / "cart.ts", CART)
    write(a / "src" / "legacy" / "coupon.ts", COUPON)
    write(b / "src" / "payments" / "promptpay.ts", PROMPTPAY)
    write(a / "config" / "app.properties", APP_14)
    write(b / "config" / "app.properties", APP_15)
    write(a / "config" / "db.yaml", DB)
    write(b / "config" / "db.yaml", DB)
    write(a / "docs" / "README.md", README_DOC, eol="\r\n")
    write(b / "docs" / "README.md", README_DOC)
    write(a / "scripts" / "deploy.sh", DEPLOY)
    write(b / "scripts" / "deploy.sh", DEPLOY, eol="\r\n")
    write(a / "i18n" / "th.json", TH_14)
    write(b / "i18n" / "th.json", TH_15)
    write(a / "data" / "products.csv", PRODUCTS_14)
    write(b / "data" / "products.csv", PRODUCTS_15)
    for side, sale, colour in [(a, "30%", (200, 70, 50)), (b, "40%", (30, 120, 90))]:
        (side / "assets").mkdir(parents=True, exist_ok=True)
        banner(side / "assets" / "banner.png", sale, colour)
    m = root / "merge"
    write(m / "base" / "shop.properties", MERGE_BASE)
    write(m / "team-a" / "shop.properties", MERGE_LEFT)
    write(m / "team-b" / "shop.properties", MERGE_RIGHT)
    fw = root / "firmware"
    fw.mkdir(parents=True, exist_ok=True)
    firmware(fw / "router-1.4.bin", 1)
    firmware(fw / "router-1.5.bin", 2)

    pairs = {
        "text": [str(a / "src" / "checkout.ts"), str(b / "src" / "checkout.ts")],
        "folder": [str(a), str(b)],
        "table": ["--table", str(a / "data" / "products.csv"), str(b / "data" / "products.csv")],
        "picture": [str(a / "assets" / "banner.png"), str(b / "assets" / "banner.png")],
        "hex": [str(fw / "router-1.4.bin"), str(fw / "router-1.5.bin")],
        "merge": [str(m / "team-a" / "shop.properties"), str(m / "team-b" / "shop.properties"),
                  str(m / "base" / "shop.properties")],
    }
    recent = [
        f"F\t{a}\t{b}",
        f"T\t{pairs['text'][0]}\t{pairs['text'][1]}",
        f"C\t{pairs['table'][1]}\t{pairs['table'][2]}",
        f"P\t{pairs['picture'][0]}\t{pairs['picture'][1]}",
        f"H\t{pairs['hex'][0]}\t{pairs['hex'][1]}",
        f"M\t{pairs['merge'][0]}\t{pairs['merge'][1]}\t{pairs['merge'][2]}",
    ]
    scenes = []
    for theme in ["dark", "light"]:
        scenes.append({"name": f"home-{theme}", "theme": theme, "args": [], "recent": recent,
                       "settings": {"start.welcome": 99}})
        for kind, args in pairs.items():
            scene = {"name": f"{kind}-{theme}", "theme": theme, "args": args, "recent": recent}
            if kind == "folder":
                scene["settings"] = {"folder.criterion": "content", "start.welcome": 99}
            else:
                scene["settings"] = {"start.welcome": 99}
            scenes.append(scene)
    Path(scenes_path).write_text(json.dumps(scenes, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"{len(scenes)} scenes -> {scenes_path}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
