#!/usr/bin/env python3
"""Build the camera-matched HeroQuest castle great hall as a game-ready GLB.

Run with:
    blender --background --python tools/build-castle-room.py

The model uses Blender's Z-up coordinate system while authoring. The GLB
exporter converts it to the Y-up coordinates used by the Rust renderer.
"""

from __future__ import annotations

import math
import random
from pathlib import Path

import bpy
from mathutils import Vector


REPO = Path(__file__).resolve().parents[1]
OUTPUT_DIR = REPO / "assets" / "local" / "environment"
GLB_PATH = OUTPUT_DIR / "castle-great-hall.glb"
BLEND_PATH = OUTPUT_DIR / "castle-great-hall.blend"
PREVIEW_PATH = OUTPUT_DIR / "castle-great-hall-preview.png"
TEXTURE_DIR = OUTPUT_DIR / "textures"


def game_point(x: float, up: float, z: float) -> tuple[float, float, float]:
    """Map the renderer's X/Y-up/Z coordinates to Blender X/Y/Z-up."""
    return (x, -z, up)


def game_scale(x: float, up: float, z: float) -> tuple[float, float, float]:
    return (x, z, up)


def clear_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for datablocks in (bpy.data.meshes, bpy.data.curves, bpy.data.materials, bpy.data.cameras, bpy.data.lights):
        for block in list(datablocks):
            if block.users == 0:
                datablocks.remove(block)


def save_texture(name: str, pixels: list[float], size: int = 128) -> bpy.types.Image:
    TEXTURE_DIR.mkdir(parents=True, exist_ok=True)
    path = TEXTURE_DIR / f"{name}.png"
    image = bpy.data.images.new(name, width=size, height=size, alpha=True)
    image.pixels.foreach_set(pixels)
    image.filepath_raw = str(path)
    image.file_format = "PNG"
    image.save()
    return image


def texture_pixels(kind: str, size: int = 128) -> list[float]:
    rng = random.Random(1989 + sum(ord(char) for char in kind))
    values: list[float] = []
    for y in range(size):
        for x in range(size):
            noise = rng.uniform(-0.045, 0.045)
            if kind == "stone":
                course = (y // 22) % 2
                joint_x = (x + (11 if course else 0)) % 36
                mortar = (y % 22 < 2) or (joint_x < 2)
                base = 0.055 if mortar else 0.19 + 0.025 * math.sin(x * 0.15 + y * 0.07)
                rgb = (base * 0.94 + noise, base * 0.90 + noise, base + noise)
            elif kind == "wood":
                grain = 0.13 * math.sin(x * 0.20 + 1.8 * math.sin(y * 0.045))
                knot = 0.055 * math.sin(math.hypot(x - 38, y - 78) * 0.42)
                rgb = (0.28 + grain + knot + noise, 0.105 + grain * 0.42 + noise * 0.4, 0.028 + noise * 0.2)
            elif kind == "floor":
                joint = (x % 32 < 2) or (y % 24 < 2)
                base = 0.045 if joint else 0.115 + 0.025 * math.sin(x * 0.08 + y * 0.11)
                rgb = (base * 0.92 + noise, base * 0.94 + noise, base + noise)
            elif kind == "rug":
                border = x < 10 or x >= size - 10 or y < 10 or y >= size - 10
                diamond = ((x + y) // 12 + (x - y) // 12) % 2 == 0
                if border:
                    rgb = (0.25 + noise, 0.17 + noise, 0.055)
                elif diamond:
                    rgb = (0.075 + noise, 0.085 + noise, 0.14 + noise)
                else:
                    rgb = (0.18 + noise, 0.035 + noise, 0.045 + noise)
            else:
                rgb = (0.5 + noise, 0.5 + noise, 0.5 + noise)
            values.extend((*[max(0.0, min(1.0, channel)) for channel in rgb], 1.0))
    return values


def material(
    name: str,
    color: tuple[float, float, float, float],
    *,
    roughness: float = 0.72,
    metallic: float = 0.0,
    texture: bpy.types.Image | None = None,
    emission: tuple[float, float, float, float] | None = None,
    emission_strength: float = 0.0,
) -> bpy.types.Material:
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    mat.diffuse_color = color
    nodes = mat.node_tree.nodes
    links = mat.node_tree.links
    principled = nodes.get("Principled BSDF")
    principled.inputs["Base Color"].default_value = color
    principled.inputs["Roughness"].default_value = roughness
    principled.inputs["Metallic"].default_value = metallic
    if texture is not None:
        tex = nodes.new("ShaderNodeTexImage")
        tex.image = texture
        tex.interpolation = "Linear"
        links.new(tex.outputs["Color"], principled.inputs["Base Color"])
    if emission is not None:
        emission_input = principled.inputs.get("Emission Color") or principled.inputs.get("Emission")
        strength_input = principled.inputs.get("Emission Strength")
        if emission_input is not None:
            emission_input.default_value = emission
        if strength_input is not None:
            strength_input.default_value = emission_strength
    return mat


def assign(obj: bpy.types.Object, mat: bpy.types.Material) -> bpy.types.Object:
    obj.data.materials.append(mat)
    return obj


def bevel(obj: bpy.types.Object, width: float, segments: int = 2) -> None:
    modifier = obj.modifiers.new("soft carved edges", "BEVEL")
    modifier.width = width
    modifier.segments = segments
    modifier.limit_method = "ANGLE"


def box(
    name: str,
    center: tuple[float, float, float],
    half: tuple[float, float, float],
    mat: bpy.types.Material,
    *,
    bevel_width: float = 0.0,
    rotation_z: float = 0.0,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cube_add(location=game_point(*center))
    obj = bpy.context.object
    obj.name = name
    obj.scale = game_scale(*half)
    obj.rotation_euler[1] = rotation_z
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel_width:
        bevel(obj, bevel_width)
    return assign(obj, mat)


def tiled_floor(
    name: str,
    up: float,
    half_extent: float,
    tile_size: float,
    mat: bpy.types.Material,
) -> bpy.types.Object:
    """Create a large perspective-correct floor with repeating world-space UVs."""
    bpy.ops.mesh.primitive_plane_add(size=2.0, location=game_point(0.0, up, 0.0))
    obj = bpy.context.object
    obj.name = name
    obj.scale = (half_extent, half_extent, 1.0)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    uv_layer = obj.data.uv_layers.active
    for loop in obj.data.loops:
        vertex = obj.data.vertices[loop.vertex_index].co
        uv_layer.data[loop.index].uv = (
            vertex.x / tile_size,
            -vertex.y / tile_size,
        )
    return assign(obj, mat)


def cylinder(
    name: str,
    center: tuple[float, float, float],
    radius: float,
    half_height: float,
    mat: bpy.types.Material,
    *,
    vertices: int = 16,
    bevel_width: float = 0.0,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius, depth=half_height * 2.0, location=game_point(*center))
    obj = bpy.context.object
    obj.name = name
    if bevel_width:
        bevel(obj, bevel_width)
    return assign(obj, mat)


def sphere(
    name: str,
    center: tuple[float, float, float],
    scale: tuple[float, float, float],
    mat: bpy.types.Material,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=2, radius=1.0, location=game_point(*center))
    obj = bpy.context.object
    obj.name = name
    obj.scale = game_scale(*scale)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return assign(obj, mat)


def beam(
    name: str,
    points: list[tuple[float, float, float]],
    radius: float,
    mat: bpy.types.Material,
    *,
    resolution: int = 2,
) -> bpy.types.Object:
    curve = bpy.data.curves.new(name, "CURVE")
    curve.dimensions = "3D"
    curve.resolution_u = 1
    curve.bevel_depth = radius
    curve.bevel_resolution = resolution
    curve.resolution_u = 2
    spline = curve.splines.new("POLY")
    spline.points.add(len(points) - 1)
    for point, target in zip(points, spline.points):
        target.co = (*game_point(*point), 1.0)
    obj = bpy.data.objects.new(name, curve)
    bpy.context.collection.objects.link(obj)
    return assign(obj, mat)


def pointed_arch_points(
    center_x: float,
    z: float,
    spring: float,
    half_width: float,
    segments: int = 10,
) -> list[tuple[float, float, float]]:
    height = half_width * math.sqrt(3.0)
    left: list[tuple[float, float, float]] = []
    for index in range(segments + 1):
        angle = math.pi - (math.pi / 3.0) * index / segments
        x = center_x + half_width + 2.0 * half_width * math.cos(angle)
        up = spring + 2.0 * half_width * math.sin(angle)
        left.append((x, up, z))
    right = [(2.0 * center_x - x, up, z) for x, up, z in reversed(left[:-1])]
    return left + right


def side_pointed_arch_points(
    x: float,
    center_z: float,
    spring: float,
    half_width: float,
    segments: int = 10,
) -> list[tuple[float, float, float]]:
    return [(x, up, center_z + (px - 0.0)) for px, up, _ in pointed_arch_points(0.0, 0.0, spring, half_width, segments)]


def vault_shell(name: str, stone: bpy.types.Material) -> bpy.types.Object:
    samples = 64
    z_values = [-178.0, 189.3]
    verts: list[tuple[float, float, float]] = []
    for z in z_values:
        for index in range(samples + 1):
            t = index / samples
            x = -159.0 + 318.0 * t
            up = 66.0 + 58.0 * (1.0 - abs(x / 159.0) ** 1.65)
            verts.append(game_point(x, up, z))
    faces = []
    row = samples + 1
    for index in range(samples):
        faces.append((index, index + 1, row + index + 1, row + index))
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    assign(obj, stone)
    solidify = obj.modifiers.new("vault thickness", "SOLIDIFY")
    solidify.thickness = 0.55
    return obj


def add_column(name: str, x: float, z: float, stone: bpy.types.Material, dark_stone: bpy.types.Material) -> None:
    cylinder(f"{name}.plinth", (x, -4.9, z), 2.0, 0.7, dark_stone, vertices=12, bevel_width=0.12)
    cylinder(f"{name}.base", (x, -3.6, z), 1.55, 0.55, stone, vertices=12, bevel_width=0.10)
    cylinder(f"{name}.shaft", (x, 15.0, z), 1.18, 18.0, stone, vertices=20, bevel_width=0.08)
    for up, radius, height in [(33.3, 1.55, 0.42), (34.2, 1.95, 0.40), (35.1, 2.35, 0.36)]:
        cylinder(f"{name}.capital.{up}", (x, up, z), radius, height, dark_stone if up != 13.5 else stone, vertices=12, bevel_width=0.08)
    for angle in range(0, 360, 90):
        radians = math.radians(angle)
        sphere(
            f"{name}.capital_leaf.{angle}",
            (x + math.cos(radians) * 1.55, 34.1, z + math.sin(radians) * 1.55),
            (0.38, 0.78, 0.27),
            dark_stone,
        )


def add_table(wood: bpy.types.Material, dark_wood: bpy.types.Material, brass: bpy.types.Material) -> None:
    # Image-calibrated proportions from the room reference: the top is about
    # 1.3x wider than deep and the floor-to-top height is about one-third of
    # its width. This also leaves a believable margin around the 26x19 board.
    # The complete physical setup adds four record sheets, spell hands, the
    # Armory, Zargon's card references, and an open Quest Book. Grow the top
    # more in depth than width so those papers sit on wood instead of hanging
    # beyond the edge while the table still reads as a long Gothic refectory.
    width_scale = 1.35
    depth_scale = 1.55
    box("table.top", (0.0, -0.45, 0.0), (18.5 * width_scale, 0.42, 14.0 * depth_scale), wood, bevel_width=0.24)
    box("table.inlay.north", (0.0, -0.055, -13.55 * depth_scale), (17.7 * width_scale, 0.035, 0.08), brass)
    box("table.inlay.south", (0.0, -0.055, 13.55 * depth_scale), (17.7 * width_scale, 0.035, 0.08), brass)
    box("table.inlay.west", (-18.05 * width_scale, -0.055, 0.0), (0.08, 0.035, 13.1 * depth_scale), brass)
    box("table.inlay.east", (18.05 * width_scale, -0.055, 0.0), (0.08, 0.035, 13.1 * depth_scale), brass)
    for z in (-13.45 * depth_scale, 13.45 * depth_scale):
        box(f"table.apron.z{z}", (0.0, -1.55, z), (16.6 * width_scale, 0.95, 0.52), dark_wood, bevel_width=0.14)
        for x in (-12.0, -6.0, 0.0, 6.0, 12.0):
            carving_x = x * width_scale
            sphere(f"table.carving.{carving_x}.{z}", (carving_x, -1.55, z - math.copysign(0.55, z)), (0.8, 0.55, 0.14), brass)
    for x in (-17.95 * width_scale, 17.95 * width_scale):
        box(f"table.apron.x{x}", (x, -1.55, 0.0), (0.52, 0.95, 11.7 * depth_scale), dark_wood, bevel_width=0.14)
    for x in (-16.2 * width_scale, 16.2 * width_scale):
        for z in (-11.7 * depth_scale, 11.7 * depth_scale):
            box(f"table.leg.{x}.{z}", (x, -7.10, z), (1.05, 4.55, 1.05), dark_wood, bevel_width=0.22)
            cylinder(f"table.leg_ring.{x}.{z}", (x, -1.25, z), 1.45, 0.28, wood, vertices=12, bevel_width=0.1)
            cylinder(f"table.foot.{x}.{z}", (x, -11.28, z), 1.55, 0.38, dark_wood, vertices=12, bevel_width=0.12)


def add_fireplace(stone: bpy.types.Material, dark_stone: bpy.types.Material, ember: bpy.types.Material, iron: bpy.types.Material) -> None:
    x = -22.0
    z = 63.0
    box("hearth.recess", (x, 1.0, z - 0.35), (6.4, 6.1, 0.34), dark_stone, bevel_width=0.10)
    for side in (-1.0, 1.0):
        column_x = x + side * 6.9
        box(f"hearth.pier.{side}", (column_x, 1.2, z - 0.95), (1.25, 6.9, 1.35), stone, bevel_width=0.20)
        cylinder(f"hearth.pier.base.{side}", (column_x, -4.6, z - 1.7), 1.8, 0.45, stone, vertices=12, bevel_width=0.10)
        cylinder(f"hearth.pier.capital.{side}", (column_x, 7.6, z - 1.0), 1.75, 0.42, stone, vertices=12, bevel_width=0.10)
    box("hearth.mantel", (x, 8.5, z - 1.0), (8.8, 0.92, 1.55), stone, bevel_width=0.22)
    box("hearth.mantel.cornice", (x, 9.7, z - 1.0), (9.8, 0.35, 1.8), dark_stone, bevel_width=0.16)
    box("hearth.slab", (x, -4.95, z - 2.7), (8.0, 0.42, 3.2), stone, bevel_width=0.16)
    beam("hearth.arch.outer", pointed_arch_points(x, z - 1.25, 1.4, 5.5, 18), 0.72, stone, resolution=3)
    beam("hearth.arch.inner", pointed_arch_points(x, z - 1.55, 1.7, 4.7, 18), 0.25, dark_stone, resolution=2)
    box("hearth.crest", (x, 13.0, z - 1.15), (3.8, 2.1, 0.50), dark_stone, bevel_width=0.20)
    for index in range(5):
        angle = math.tau * index / 5.0
        sphere(f"hearth.crest.device.{index}", (x + math.cos(angle) * 2.0, 13.0 + math.sin(angle) * 1.15, z - 1.72), (0.38, 0.38, 0.14), stone)
    for offset, angle in [(-2.4, -0.25), (-0.8, 0.15), (0.8, -0.1), (2.4, 0.22)]:
        log = cylinder(f"hearth.log.{offset}", (x + offset, -3.55, z - 3.3), 0.38, 2.2, dark_stone, vertices=10)
        log.rotation_euler[1] = math.pi / 2.0 + angle
    for index, (flame_x, height) in enumerate([(x - 3.0, 3.1), (x - 1.5, 5.2), (x, 6.4), (x + 1.6, 4.8), (x + 3.1, 3.4)]):
        flame = sphere(f"hearth.flame.{index}", (flame_x, -2.2 + height * 0.34, z - 3.5), (0.58, height * 0.52, 0.30), ember)
        flame.rotation_euler[1] = (index - 1.5) * 0.13
    for andiron_x in (x - 4.0, x + 4.0):
        cylinder(f"hearth.andiron.{andiron_x}", (andiron_x, -2.5, z - 3.45), 0.20, 1.45, iron, vertices=8)


def add_tapestry(name: str, center: tuple[float, float, float], half: tuple[float, float, float], cloth: bpy.types.Material, gold: bpy.types.Material, side: bool = False) -> None:
    box(name, center, half, cloth, bevel_width=0.06)
    x, up, z = center
    if side:
        box(f"{name}.border.top", (x - math.copysign(0.03, x), up + half[1] - 0.25, z), (0.07, 0.14, half[2] * 0.88), gold)
        box(f"{name}.border.bottom", (x - math.copysign(0.03, x), up - half[1] + 0.25, z), (0.07, 0.14, half[2] * 0.88), gold)
        sphere(f"{name}.device", (x - math.copysign(0.12, x), up, z), (0.10, 1.4, 1.15), gold)
    else:
        box(f"{name}.border.left", (x - half[0] + 0.25, up, z - 0.08), (0.14, half[1] * 0.88, 0.07), gold)
        box(f"{name}.border.right", (x + half[0] - 0.25, up, z - 0.08), (0.14, half[1] * 0.88, 0.07), gold)
        sphere(f"{name}.device", (x, up, z - 0.16), (1.15, 1.4, 0.10), gold)


def add_brazier(name: str, x: float, z: float, iron: bpy.types.Material, ember: bpy.types.Material) -> None:
    cylinder(f"{name}.stem", (x, -2.5, z), 0.22, 2.7, iron, vertices=8)
    cylinder(f"{name}.foot", (x, -5.25, z), 1.0, 0.18, iron, vertices=12, bevel_width=0.08)
    bpy.ops.mesh.primitive_torus_add(major_radius=1.2, minor_radius=0.16, major_segments=16, minor_segments=6, location=game_point(x, 0.0, z))
    ring = bpy.context.object
    ring.name = f"{name}.basket"
    assign(ring, iron)
    for index in range(8):
        angle = math.tau * index / 8
        beam(
            f"{name}.rib.{index}",
            [
                (x + math.cos(angle) * 0.35, -0.1, z + math.sin(angle) * 0.35),
                (x + math.cos(angle) * 1.15, 1.6, z + math.sin(angle) * 1.15),
            ],
            0.10,
            iron,
            resolution=1,
        )
    for index, (dx, dz, height) in enumerate([(-0.45, 0.0, 2.2), (0.35, -0.2, 3.0), (0.1, 0.4, 2.5)]):
        sphere(f"{name}.flame.{index}", (x + dx, 1.25 + height * 0.35, z + dz), (0.38, height * 0.45, 0.30), ember)


def add_chandelier(iron: bpy.types.Material, candle: bpy.types.Material) -> None:
    # Keep the ring behind the tabletop footprint so even the steepest legal
    # gameplay camera cannot place it between the player and the board.
    center = (6.0, 34.0, 29.0)
    for offset in (-3.0, 0.0, 3.0):
        beam(f"chandelier.chain.{offset}", [(6.0 + offset, 62.0, 29.0), (6.0 + offset * 0.45, 36.0, 29.0)], 0.13, iron, resolution=1)
    bpy.ops.mesh.primitive_torus_add(major_radius=5.8, minor_radius=0.28, major_segments=40, minor_segments=8, location=game_point(*center))
    ring = bpy.context.object
    ring.name = "chandelier.ring"
    assign(ring, iron)
    for index in range(16):
        angle = math.tau * index / 16
        x = 6.0 + math.cos(angle) * 5.8
        z = 29.0 + math.sin(angle) * 5.8
        cylinder(f"chandelier.candle.{index}", (x, 34.85, z), 0.14, 0.78, iron, vertices=8)
        sphere(f"chandelier.flame.{index}", (x, 35.88, z), (0.18, 0.48, 0.18), candle)
        beam(f"chandelier.spoke.{index}", [center, (x, 34.0, z)], 0.10, iron, resolution=1)


def add_windows(moon: bpy.types.Material, stone: bpy.types.Material) -> None:
    for index, z in enumerate((25.0, 1.0, -23.0)):
        x = 52.25
        box(f"window.{index}.glass", (x - 0.15, 15.0, z), (0.08, 10.5, 5.8), moon, bevel_width=0.04)
        beam(f"window.{index}.arch.outer", side_pointed_arch_points(x - 0.35, z, 15.0, 6.0, 18), 0.62, stone, resolution=3)
        beam(f"window.{index}.arch.inner", side_pointed_arch_points(x - 0.48, z, 15.4, 5.2, 18), 0.22, stone, resolution=2)
        box(f"window.{index}.mullion", (x - 0.42, 13.1, z), (0.18, 8.2, 0.22), stone)
        box(f"window.{index}.transom", (x - 0.42, 12.5, z), (0.18, 0.20, 5.1), stone)
        for offset in (-2.7, 2.7):
            box(f"window.{index}.lancet.{offset}", (x - 0.44, 13.2, z + offset), (0.16, 7.8, 0.13), stone)


def add_back_rosette(name: str, x: float, up: float, z: float, radius: float, stone: bpy.types.Material, iron: bpy.types.Material) -> None:
    bpy.ops.mesh.primitive_torus_add(
        major_radius=radius,
        minor_radius=0.34,
        major_segments=32,
        minor_segments=8,
        location=game_point(x, up, z),
        rotation=(math.pi / 2.0, 0.0, 0.0),
    )
    ring = bpy.context.object
    ring.name = f"{name}.ring"
    assign(ring, stone)
    for index in range(12):
        angle = math.tau * index / 12.0
        beam(
            f"{name}.spoke.{index}",
            [(x, up, z), (x + math.cos(angle) * radius, up + math.sin(angle) * radius, z)],
            0.13,
            iron,
            resolution=1,
        )
    sphere(f"{name}.boss", (x, up, z - 0.25), (0.58, 0.58, 0.22), stone)


def add_masonry_relief(stone: bpy.types.Material, dark_stone: bpy.types.Material) -> None:
    for course in range(10):
        up = -2.7 + course * 3.9
        offset = 4.0 if course % 2 else 0.0
        for block_index in range(-7, 7):
            x = block_index * 8.0 + offset
            if -50.0 < x < 50.0:
                box(f"back.block.{course}.{block_index}", (x, up, 63.28), (3.72, 1.72, 0.16), stone)
    for side in (-1.0, 1.0):
        for course in range(10):
            up = -2.7 + course * 3.9
            offset = 4.0 if course % 2 else 0.0
            for block_index in range(-8, 8):
                z = block_index * 8.0 + offset + 5.0
                if -56.0 < z < 63.0:
                    box(f"side.block.{side}.{course}.{block_index}", (side * 52.28, up, z), (0.16, 1.72, 3.72), stone)
    for up, height in [(5.0, 0.35), (19.0, 0.42), (37.0, 0.55)]:
        box(f"back.string-course.{up}", (0.0, up, 62.88), (51.5, height, 0.38), dark_stone, bevel_width=0.08)
        for side in (-1.0, 1.0):
            box(f"side.string-course.{side}.{up}", (side * 51.88, up, 5.0), (0.38, height, 57.0), dark_stone, bevel_width=0.08)


def add_wall_sconce(name: str, x: float, up: float, z: float, iron: bpy.types.Material, candle: bpy.types.Material) -> None:
    beam(f"{name}.arm", [(x, up, z), (x, up + 0.4, z - 1.5)], 0.13, iron, resolution=1)
    cylinder(f"{name}.cup", (x, up + 0.7, z - 1.7), 0.45, 0.18, iron, vertices=10)
    cylinder(f"{name}.candle", (x, up + 1.3, z - 1.7), 0.12, 0.58, iron, vertices=8)
    sphere(f"{name}.flame", (x, up + 2.15, z - 1.7), (0.16, 0.44, 0.16), candle)


def add_armor_statue(
    name: str,
    x: float,
    z: float,
    stone: bpy.types.Material,
    iron: bpy.types.Material,
    brass: bpy.types.Material,
) -> None:
    box(f"{name}.plinth", (x, -4.65, z), (1.75, 0.85, 1.75), stone, bevel_width=0.16)
    for side in (-0.55, 0.55):
        cylinder(f"{name}.leg.{side}", (x + side, -2.7, z), 0.28, 1.25, iron, vertices=10)
    box(f"{name}.cuirass", (x, 0.0, z), (1.18, 1.65, 0.62), iron, bevel_width=0.20)
    box(f"{name}.shoulders", (x, 1.1, z), (1.65, 0.35, 0.72), brass, bevel_width=0.12)
    sphere(f"{name}.helm", (x, 2.35, z), (0.72, 0.86, 0.68), iron)
    beam(f"{name}.spear", [(x + 1.35, -3.5, z), (x + 1.35, 5.5, z)], 0.10, brass, resolution=1)
    box(f"{name}.shield", (x - 1.15, -0.1, z - 0.75), (0.80, 1.55, 0.16), brass, bevel_width=0.18)
    sphere(f"{name}.shield.device", (x - 1.15, -0.1, z - 0.95), (0.28, 0.28, 0.12), iron)


def add_rear_balustrade(dark_wood: bpy.types.Material, wood: bpy.types.Material, brass: bpy.types.Material) -> None:
    z = 28.0
    box("rear-balustrade.lower", (0.0, -3.65, z), (15.5, 0.55, 0.72), dark_wood, bevel_width=0.16)
    box("rear-balustrade.upper", (0.0, 2.15, z), (16.2, 0.42, 0.82), wood, bevel_width=0.18)
    for index, x in enumerate((-14.5, -10.5, -6.0, -2.0, 2.0, 6.0, 10.5, 14.5)):
        box(f"rear-balustrade.post.{index}", (x, -0.7, z), (0.46, 2.55, 0.48), dark_wood, bevel_width=0.15)
        sphere(f"rear-balustrade.finial.{index}", (x, 2.95, z), (0.60, 0.78, 0.60), brass)
    for index, x in enumerate((-12.5, -8.25, -4.0, 0.0, 4.0, 8.25, 12.5)):
        box(f"rear-balustrade.panel.{index}", (x, -0.65, z - 0.18), (1.62, 2.15, 0.16), wood, bevel_width=0.12)
        sphere(f"rear-balustrade.carving.{index}", (x, -0.65, z - 0.42), (0.72, 0.92, 0.12), brass)


def build_room() -> None:
    stone_image = save_texture("gothic-stone", texture_pixels("stone"))
    wood_image = save_texture("dark-oak", texture_pixels("wood"))
    floor_image = save_texture("flagstone", texture_pixels("floor"))
    rug_image = save_texture("heraldic-rug", texture_pixels("rug"))

    stone = material("Gothic Stone", (0.24, 0.22, 0.24, 1.0), texture=stone_image, roughness=0.92)
    dark_stone = material("Recess Stone", (0.055, 0.045, 0.055, 1.0), roughness=0.96)
    floor_mat = material("Wet Flagstone", (0.62, 0.60, 0.68, 1.0), texture=floor_image, roughness=0.80)
    wood = material("Carved Oak", (0.34, 0.13, 0.035, 1.0), texture=wood_image, roughness=0.62)
    dark_wood = material("Dark Oak", (0.12, 0.035, 0.012, 1.0), texture=wood_image, roughness=0.70)
    iron = material("Black Iron", (0.025, 0.022, 0.028, 1.0), roughness=0.31, metallic=0.88)
    brass = material("Aged Gold", (0.42, 0.23, 0.055, 1.0), roughness=0.42, metallic=0.72)
    rug = material("Heraldic Rug", (0.13, 0.035, 0.06, 1.0), texture=rug_image, roughness=0.92)
    red_cloth = material("Crimson Tapestry", (0.27, 0.022, 0.035, 1.0), roughness=0.95)
    blue_cloth = material("Midnight Tapestry", (0.025, 0.055, 0.17, 1.0), roughness=0.95)
    ember = material("Fire", (1.0, 0.16, 0.01, 1.0), roughness=0.25, emission=(1.0, 0.055, 0.003, 1.0), emission_strength=7.0)
    candle = material("Candle Flame", (1.0, 0.55, 0.06, 1.0), roughness=0.18, emission=(1.0, 0.22, 0.01, 1.0), emission_strength=4.0)
    moon = material("Moonlit Glass", (0.10, 0.25, 0.48, 1.0), roughness=0.18, emission=(0.06, 0.18, 0.55, 1.0), emission_strength=2.0)
    # The panorama supplies distant walls and ceiling only. A very large real
    # floor supplies perspective, parallax, and depth beneath every camera
    # angle, with world-scaled UVs so the stones do not stretch into giant slabs.
    tiled_floor("floor", -11.66, 380.0, 18.0, floor_mat)
    box("rug", (0.0, -11.61, 0.0), (21.5, 0.045, 16.5), rug, bevel_width=0.05)

    # Only near-field structure is modeled. The complete hall is supplied by a
    # screen-space camera plate in the renderer, avoiding both nested walls and
    # the perspective error of putting a painted floor on a vertical plane.

    add_table(wood, dark_wood, brass)


def add_preview_camera_and_lights() -> None:
    bpy.ops.object.camera_add(location=game_point(48.0, 29.0, -56.0))
    camera = bpy.context.object
    camera.name = "Reference Gameplay Camera"
    direction = Vector(game_point(0.0, -2.5, 3.0)) - camera.location
    camera.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    camera.data.lens = 31.0
    bpy.context.scene.camera = camera

    def point_light(name: str, center: tuple[float, float, float], color: tuple[float, float, float], energy: float, radius: float) -> None:
        bpy.ops.object.light_add(type="POINT", location=game_point(*center))
        lamp = bpy.context.object
        lamp.name = name
        lamp.data.color = color
        lamp.data.energy = energy
        lamp.data.shadow_soft_size = radius

    point_light("Hearth Light", (-22.0, 2.0, 59.0), (1.0, 0.16, 0.035), 22000.0, 5.0)
    point_light("Chandelier Light", (6.0, 35.0, 29.0), (1.0, 0.42, 0.12), 12000.0, 6.0)
    point_light("Moon Window Light", (47.0, 18.0, 9.0), (0.11, 0.26, 1.0), 26000.0, 8.0)
    point_light("West Brazier Light", (-44.0, 2.0, 32.0), (1.0, 0.20, 0.035), 9000.0, 4.0)
    point_light("East Brazier Light", (44.0, 2.0, 32.0), (1.0, 0.20, 0.035), 9000.0, 4.0)

    bpy.ops.object.light_add(type="AREA", location=game_point(0.0, 40.0, 72.0))
    plate_light = bpy.context.object
    plate_light.name = "Projection Plate Fill"
    plate_light.data.energy = 4200.0
    plate_light.data.shape = "RECTANGLE"
    plate_light.data.size = 120.0
    plate_direction = Vector(game_point(0.0, 35.0, 92.0)) - plate_light.location
    plate_light.rotation_euler = plate_direction.to_track_quat("-Z", "Y").to_euler()

    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 1280
    scene.render.resolution_y = 720
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(PREVIEW_PATH)
    scene.render.film_transparent = False
    scene.world.use_nodes = True
    background = scene.world.node_tree.nodes.get("Background")
    background.inputs["Color"].default_value = (0.012, 0.016, 0.032, 1.0)
    background.inputs["Strength"].default_value = 0.11
    scene.view_settings.look = "AgX - Medium High Contrast"
    scene.view_settings.exposure = 1.10


def apply_exportable_modifiers() -> None:
    bpy.ops.object.select_all(action="DESELECT")
    bpy.context.view_layer.objects.active = None
    for obj in list(bpy.context.scene.objects):
        if obj.type not in {"MESH", "CURVE"}:
            continue
        bpy.ops.object.select_all(action="DESELECT")
        obj.select_set(True)
        bpy.context.view_layer.objects.active = obj
        if obj.type == "CURVE":
            bpy.ops.object.convert(target="MESH")
        for modifier in list(obj.modifiers):
            bpy.ops.object.modifier_apply(modifier=modifier.name)
        bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
        obj.select_set(False)


def join_meshes_by_material() -> None:
    groups: dict[str, list[bpy.types.Object]] = {}
    for obj in bpy.context.scene.objects:
        if obj.type != "MESH" or not obj.data.materials:
            continue
        material_name = obj.data.materials[0].name
        groups.setdefault(material_name, []).append(obj)
    for material_name, objects in groups.items():
        if len(objects) == 1:
            objects[0].name = f"room.{material_name.lower().replace(' ', '-')}"
            continue
        bpy.ops.object.select_all(action="DESELECT")
        for obj in objects:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = objects[0]
        bpy.ops.object.join()
        objects[0].name = f"room.{material_name.lower().replace(' ', '-')}"


def main() -> None:
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    clear_scene()
    build_room()
    add_preview_camera_and_lights()
    bpy.ops.wm.save_as_mainfile(filepath=str(BLEND_PATH))
    bpy.ops.render.render(write_still=True)
    apply_exportable_modifiers()
    join_meshes_by_material()
    bpy.ops.export_scene.gltf(
        filepath=str(GLB_PATH),
        export_format="GLB",
        export_yup=True,
        export_materials="EXPORT",
        export_cameras=False,
        export_lights=False,
        export_apply=True,
    )
    print(f"Wrote {GLB_PATH}")
    print(f"Wrote {BLEND_PATH}")
    print(f"Wrote {PREVIEW_PATH}")


if __name__ == "__main__":
    main()
