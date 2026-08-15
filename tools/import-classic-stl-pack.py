#!/usr/bin/env python3
"""Convert the audited private classic STL pack into optimized runtime GLBs.

Run through Blender, for example:
  blender --background --python tools/import-classic-stl-pack.py -- \
    --source-root /tmp/heroquest-model-audit \
    --output-root assets/local/editions/original-us/models

The output directory is intentionally under the gitignored private asset pack.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path

import bpy


@dataclass(frozen=True)
class Model:
    output: str
    inputs: tuple[str, ...]
    triangle_budget: int


MODELS = (
    Model("figures/barbarian.glb", ("Minis/BarbarianNewBase.stl",), 120_000),
    Model("figures/dwarf.glb", ("Minis/DwarfNewBase.stl",), 120_000),
    Model("figures/elf.glb", ("Minis/ElfNewBase.stl",), 120_000),
    Model("figures/wizard.glb", ("Minis/WizardNewBase.stl",), 120_000),
    Model("figures/goblin-sword.glb", ("Minis/GoblinSwordNewBase.stl",), 105_000),
    Model("figures/goblin-axe.glb", ("Minis/GoblinAxeNewBase.stl",), 105_000),
    Model("figures/goblin-scimitar.glb", ("Minis/GoblinDaggerNewBase.stl",), 105_000),
    Model("figures/orc-sword.glb", ("Minis/OrcSwordNewBase.stl",), 120_000),
    Model("figures/orc-flail.glb", ("Minis/OrcMaceNewBase.stl",), 120_000),
    Model("figures/orc-cleaver.glb", ("Minis/OrcCleaverNewBase.stl",), 120_000),
    Model("figures/fimir.glb", ("Minis/FimirNewBase.stl",), 120_000),
    Model("figures/chaos-warrior.glb", ("Minis/ChaosWarriorNewbase.stl",), 120_000),
    Model("figures/chaos-warlock.glb", ("Minis/ChaosMageNewBase.stl",), 120_000),
    Model(
        "figures/gargoyle.glb",
        ("Minis/GargoyleNewBase.stl", "Minis/GargoyleWings.stl"),
        180_000,
    ),
    Model("figures/skeleton.glb", ("Minis/SkeletonNewBase.stl",), 110_000),
    Model("figures/zombie.glb", ("Minis/ZombieNewBase.stl",), 110_000),
    Model("figures/mummy.glb", ("Minis/MummyNewBase.stl",), 110_000),
    Model("furniture/table.glb", ("Furniture/Just_Table.stl",), 80_000),
    Model("furniture/treasure-chest.glb", ("Furniture/Chest_forFDM.stl",), 100_000),
    Model(
        "furniture/bookcase.glb",
        (
            "Furniture/Bookcase_NoShelf_Full_Textured.stl",
            "Furniture/Bookcase_OnlyShelf.stl",
        ),
        120_000,
    ),
    Model("furniture/throne.glb", ("Furniture/Throne_HeroQuest.stl",), 100_000),
    Model("furniture/alchemists-bench.glb", ("Furniture/AlchemyTable.stl",), 130_000),
    Model(
        "furniture/tomb.glb",
        ("Furniture/Tomb_Bottom.stl", "Furniture/Tomb_Top.stl"),
        130_000,
    ),
    Model("furniture/sorcerers-table.glb", ("Furniture/SorcererTable_FDM.stl",), 110_000),
    Model("furniture/torture-rack.glb", ("Furniture/TotureRack_full.stl",), 140_000),
    Model("furniture/fireplace.glb", ("Furniture/Fireplace_forFDM.stl",), 100_000),
    Model("furniture/cupboard.glb", ("Furniture/Buffet_ForFDM.stl",), 110_000),
    Model("dice/combat-reference.glb", ("Dice/HQdice.STL",), 80_000),
    Model("markers/pit-trap.glb", ("Tiles/PitTrap2.stl",), 80_000),
    Model("markers/falling-block-trap.glb", ("Tiles/WeakFloorTile.stl",), 80_000),
    Model("markers/secret-door.glb", ("Tiles/TrapDoorTile_solid.stl",), 80_000),
)


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    return parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])


def clear_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for block in list(bpy.data.meshes):
        if block.users == 0:
            bpy.data.meshes.remove(block)


def import_stl(path: Path) -> list[bpy.types.Object]:
    before = set(bpy.data.objects)
    bpy.ops.wm.stl_import(filepath=str(path))
    return [obj for obj in bpy.data.objects if obj not in before and obj.type == "MESH"]


def optimize(objects: list[bpy.types.Object], triangle_budget: int) -> tuple[int, int]:
    before = sum(len(obj.data.polygons) for obj in objects)
    ratio = min(1.0, triangle_budget / max(1, before))
    for obj in objects:
        if ratio < 0.999:
            modifier = obj.modifiers.new(name="Runtime decimation", type="DECIMATE")
            modifier.ratio = ratio
            modifier.use_collapse_triangulate = True
            bpy.context.view_layer.objects.active = obj
            bpy.ops.object.modifier_apply(modifier=modifier.name)
        for polygon in obj.data.polygons:
            polygon.use_smooth = True
    after = sum(len(obj.data.polygons) for obj in objects)
    return before, after


def export_model(source_root: Path, output_root: Path, model: Model) -> dict[str, object]:
    paths = [source_root / relative for relative in model.inputs]
    missing = [path for path in paths if not path.is_file()]
    if missing:
        return {
            "output": model.output,
            "status": "missing-source",
            "missing": [str(path) for path in missing],
        }

    clear_scene()
    objects: list[bpy.types.Object] = []
    for path in paths:
        objects.extend(import_stl(path))
    if not objects:
        raise RuntimeError(f"no mesh objects imported for {model.output}")

    before, after = optimize(objects, model.triangle_budget)
    for obj in bpy.context.selected_objects:
        obj.select_set(False)
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]

    target = output_root / model.output
    target.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=str(target),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_normals=True,
        export_materials="NONE",
        export_apply=True,
    )
    return {
        "output": model.output,
        "status": "ready",
        "inputs": list(model.inputs),
        "triangles_before": before,
        "triangles_after": after,
        "bytes": target.stat().st_size,
    }


def main() -> None:
    args = arguments()
    args.output_root.mkdir(parents=True, exist_ok=True)
    results = [export_model(args.source_root, args.output_root, model) for model in MODELS]
    receipt = {
        "source": "greengreenwine_community_pack",
        "source_url": "https://drive.google.com/drive/folders/1t-CAUKRnYzHFjuWopB0L0RKoyBeyW-MD",
        "redistribution": "prohibited-until-per-file-license-review",
        "models": results,
    }
    receipt_path = args.output_root / "private-import-receipt.json"
    receipt_path.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    ready = sum(result["status"] == "ready" for result in results)
    print(f"Imported {ready}/{len(results)} private classic model assets")
    print(f"Receipt: {receipt_path}")


if __name__ == "__main__":
    main()
