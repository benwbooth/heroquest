#!/usr/bin/env python3
"""Render a front three-quarter inspection image for a GLB, OBJ, or STL."""

from __future__ import annotations

import argparse
import math
import sys
from pathlib import Path

import bpy
from mathutils import Vector


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("model", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--azimuth",
        type=float,
        default=3.35,
        help="camera angle in degrees around the model; zero looks from -Y",
    )
    parser.add_argument(
        "--elevation",
        type=float,
        default=0.06,
        help="camera height above the model center, measured in model heights",
    )
    return parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])


def main() -> None:
    args = arguments()
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    match args.model.suffix.lower():
        case ".glb" | ".gltf":
            bpy.ops.import_scene.gltf(filepath=str(args.model))
        case ".obj":
            if hasattr(bpy.ops.wm, "obj_import"):
                bpy.ops.wm.obj_import(filepath=str(args.model))
            else:
                bpy.ops.import_scene.obj(filepath=str(args.model))
        case ".stl":
            if hasattr(bpy.ops.wm, "stl_import"):
                bpy.ops.wm.stl_import(filepath=str(args.model))
            else:
                bpy.ops.import_mesh.stl(filepath=str(args.model))
        case suffix:
            raise RuntimeError(f"unsupported model extension: {suffix}")
    meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    if not meshes:
        raise RuntimeError(f"no mesh found in {args.model}")

    green = bpy.data.materials.new("HeroQuest green plastic")
    green.diffuse_color = (0.18, 0.48, 0.12, 1.0)
    green.metallic = 0.0
    green.roughness = 0.4
    for obj in meshes:
        obj.data.materials.clear()
        obj.data.materials.append(green)

    bounds = [obj.matrix_world @ Vector(corner) for obj in meshes for corner in obj.bound_box]
    minimum = Vector((min(v.x for v in bounds), min(v.y for v in bounds), min(v.z for v in bounds)))
    maximum = Vector((max(v.x for v in bounds), max(v.y for v in bounds), max(v.z for v in bounds)))
    center = (minimum + maximum) * 0.5
    height = maximum.z - minimum.z
    extent = max(maximum.x - minimum.x, maximum.y - minimum.y, height)

    azimuth = math.radians(args.azimuth)
    radius = extent * 2.05
    bpy.ops.object.camera_add(
        location=(
            center.x + math.sin(azimuth) * radius,
            center.y - math.cos(azimuth) * radius,
            center.z + extent * args.elevation,
        )
    )
    camera = bpy.context.object
    camera.data.lens = 68.0
    camera.rotation_euler = (center - camera.location).to_track_quat("-Z", "Y").to_euler()
    bpy.context.scene.camera = camera

    scene = bpy.context.scene
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.studio_light = "paint.sl"
    scene.display.shading.color_type = "MATERIAL"
    scene.display.shading.show_shadows = True
    scene.display.shading.show_cavity = True
    scene.display.shading.cavity_type = "BOTH"
    scene.display.shading.curvature_ridge_factor = 1.5
    scene.display.shading.curvature_valley_factor = 1.1
    scene.render.resolution_x = 768
    scene.render.resolution_y = 768
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.film_transparent = False
    scene.world.color = (0.012, 0.012, 0.016)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    scene.render.filepath = str(args.output)
    bpy.ops.render.render(write_still=True)
    print(f"Rendered {args.output}")


if __name__ == "__main__":
    main()
