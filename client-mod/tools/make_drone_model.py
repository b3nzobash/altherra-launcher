#!/usr/bin/env python3
"""Генерує 3D-моделі FPV-дрона (звичайний і з ТНТ) для Altherra Client. Текстури — ванільні, без своїх PNG.
Запуск із кореня репозиторію: python3 tools/make_drone_model.py"""
import json
import os

ASSETS = "src/main/resources/assets/altherra"


def cube(frm, to, tex, rotation=None, faces=("north", "south", "east", "west", "up", "down")):
    e = {"from": frm, "to": to, "faces": {f: {"texture": "#" + (tex[f] if isinstance(tex, dict) else tex)} for f in faces}}
    if rotation:
        e["rotation"] = rotation
    return e


def drone(tnt):
    els = []
    # Корпус і «кришка».
    els.append(cube([5, 7, 4.5], [11, 8.5, 11.5], "body"))
    els.append(cube([6, 8.5, 5.5], [10, 9.3, 10.5], "frame"))
    # Камера спереду (північ — напрямок польоту) з лінзою.
    els.append(cube([6.5, 7.3, 3], [9.5, 9.3, 4.5], "frame"))
    els.append(cube([7.2, 7.8, 2.6], [8.8, 8.9, 3], "lens", faces=("north",)))
    # Чотири промені навхрест, повернуті на 45°.
    for angle in (45, -45):
        els.append(cube([1, 7.5, 7.4], [15, 8.2, 8.6], "frame", {"angle": angle, "axis": "y", "origin": [8, 8, 8]}))
    # Мотори й пропелери на кінцях променів (відстань 4.95 від центру по діагоналі).
    for dx in (-4.95, 4.95):
        for dz in (-4.95, 4.95):
            cx, cz = 8 + dx, 8 + dz
            els.append(cube([cx - 0.9, 8.2, cz - 0.9], [cx + 0.9, 9.6, cz + 0.9], "motor"))
            els.append(cube([cx - 3, 9.6, cz - 0.4], [cx + 3, 9.8, cz + 0.4], "prop",
                            {"angle": 22.5 if dx * dz > 0 else -22.5, "axis": "y", "origin": [cx, 9.7, cz]}))
    # Червоні вогники ззаду.
    els.append(cube([5.5, 7.4, 11.5], [6.5, 8.2, 11.9], "led"))
    els.append(cube([9.5, 7.4, 11.5], [10.5, 8.2, 11.9], "led"))
    textures = {
        "particle": "minecraft:block/gray_concrete",
        "body": "minecraft:block/gray_concrete",
        "frame": "minecraft:block/black_concrete",
        "lens": "minecraft:block/light_blue_stained_glass",
        "motor": "minecraft:block/iron_block",
        "prop": "minecraft:block/light_gray_concrete",
        "led": "minecraft:block/redstone_block",
    }
    if tnt:
        # ТНТ, підвішений під корпусом.
        textures.update({"tnt_side": "minecraft:block/tnt_side", "tnt_top": "minecraft:block/tnt_top", "tnt_bottom": "minecraft:block/tnt_bottom"})
        side = {"north": "tnt_side", "south": "tnt_side", "east": "tnt_side", "west": "tnt_side", "up": "tnt_top", "down": "tnt_bottom"}
        els.append(cube([5.75, 3.5, 5.25], [10.25, 7, 10.75], side))
    return {
        "textures": textures,
        "elements": els,
        "display": {
            "gui": {"rotation": [30, 225, 0], "translation": [0, 1, 0], "scale": [0.75, 0.75, 0.75]},
            "ground": {"translation": [0, 2, 0], "scale": [0.5, 0.5, 0.5]},
            "fixed": {"scale": [1, 1, 1]},
            "thirdperson_righthand": {"rotation": [75, 45, 0], "translation": [0, 2.5, 0], "scale": [0.45, 0.45, 0.45]},
            "thirdperson_lefthand": {"rotation": [75, 45, 0], "translation": [0, 2.5, 0], "scale": [0.45, 0.45, 0.45]},
            "firstperson_righthand": {"rotation": [0, 45, 0], "translation": [0, 2, 0], "scale": [0.5, 0.5, 0.5]},
            "firstperson_lefthand": {"rotation": [0, 225, 0], "translation": [0, 2, 0], "scale": [0.5, 0.5, 0.5]},
            "head": {"translation": [0, 6, 0], "scale": [0.8, 0.8, 0.8]},
        },
    }


for name, tnt in (("fpv_drone", False), ("fpv_drone_tnt", True)):
    os.makedirs(f"{ASSETS}/models/item", exist_ok=True)
    os.makedirs(f"{ASSETS}/items", exist_ok=True)
    with open(f"{ASSETS}/models/item/{name}.json", "w") as f:
        json.dump(drone(tnt), f, indent=1)
    # Визначення предмета (1.21.4+): яку модель малювати для item_model "altherra:<name>".
    with open(f"{ASSETS}/items/{name}.json", "w") as f:
        json.dump({"model": {"type": "minecraft:model", "model": f"altherra:item/{name}"}}, f, indent=1)
print("ok")
