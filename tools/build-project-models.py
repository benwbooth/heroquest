#!/usr/bin/env python3
"""Build project-authored pieces that do not require third-party STL files."""

from __future__ import annotations

import argparse
import math
import sys
from pathlib import Path

import bpy
from mathutils import Vector


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-root", type=Path, required=True)
    return parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])


def clear_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)


def cube(name: str, location: tuple[float, float, float], scale: tuple[float, float, float]):
    bpy.ops.mesh.primitive_cube_add(location=location)
    obj = bpy.context.object
    obj.name = name
    obj.scale = scale
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return obj


def cylinder(
    name: str,
    location: tuple[float, float, float],
    radius: float,
    depth: float,
    rotation: tuple[float, float, float] = (0.0, 0.0, 0.0),
    vertices: int = 20,
):
    bpy.ops.mesh.primitive_cylinder_add(
        vertices=vertices,
        radius=radius,
        depth=depth,
        location=location,
        rotation=rotation,
    )
    obj = bpy.context.object
    obj.name = name
    return obj


def uv_sphere(
    name: str,
    location: tuple[float, float, float],
    scale: tuple[float, float, float],
    segments: int = 40,
    rings: int = 24,
):
    bpy.ops.mesh.primitive_uv_sphere_add(
        segments=segments,
        ring_count=rings,
        location=location,
    )
    obj = bpy.context.object
    obj.name = name
    obj.scale = scale
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    for polygon in obj.data.polygons:
        polygon.use_smooth = True
    return obj


def cylinder_between(
    name: str,
    start: tuple[float, float, float],
    end: tuple[float, float, float],
    radius: float,
    vertices: int = 16,
):
    start_vector = Vector(start)
    end_vector = Vector(end)
    direction = end_vector - start_vector
    obj = cylinder(
        name,
        tuple((start_vector + end_vector) * 0.5),
        radius,
        direction.length,
        vertices=vertices,
    )
    obj.rotation_euler = direction.to_track_quat("Z", "Y").to_euler()
    return obj


def cone_between(
    name: str,
    start: tuple[float, float, float],
    end: tuple[float, float, float],
    start_radius: float,
    end_radius: float,
    vertices: int = 24,
):
    start_vector = Vector(start)
    end_vector = Vector(end)
    direction = end_vector - start_vector
    bpy.ops.mesh.primitive_cone_add(
        vertices=vertices,
        radius1=start_radius,
        radius2=end_radius,
        depth=direction.length,
        location=tuple((start_vector + end_vector) * 0.5),
    )
    obj = bpy.context.object
    obj.name = name
    obj.rotation_euler = direction.to_track_quat("Z", "Y").to_euler()
    return obj


def bevel_object(obj: bpy.types.Object, width: float, segments: int = 3) -> None:
    modifier = obj.modifiers.new(name="soft molded edge", type="BEVEL")
    modifier.width = width
    modifier.segments = segments
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier=modifier.name)


def subtract(target: bpy.types.Object, cutter: bpy.types.Object) -> None:
    modifier = target.modifiers.new(name=f"cut {cutter.name}", type="BOOLEAN")
    modifier.operation = "DIFFERENCE"
    modifier.solver = "EXACT"
    modifier.object = cutter
    bpy.context.view_layer.objects.active = target
    bpy.ops.object.modifier_apply(modifier=modifier.name)
    bpy.data.objects.remove(cutter, do_unlink=True)


def export(output_root: Path, relative: str) -> None:
    target = output_root / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.export_scene.gltf(
        filepath=str(target),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_normals=True,
        export_materials="NONE",
        export_apply=True,
    )


def build_weapon_rack(output_root: Path) -> None:
    clear_scene()
    cube("rack base", (0.0, 0.0, 1.0), (18.0, 4.0, 1.0))
    for x in (-15.0, 15.0):
        cube("upright", (x, 0.0, 22.0), (2.2, 2.2, 22.0))
        cube("foot", (x, 0.0, 1.6), (5.5, 7.0, 1.6))
    for z in (14.0, 34.0):
        cube("crossbar", (0.0, 0.0, z), (17.0, 2.2, 2.2))
    for index, x in enumerate((-10.5, -3.5, 3.5, 10.5)):
        cylinder("weapon shaft", (x, -3.5, 24.0), 0.9, 35.0)
        if index % 2 == 0:
            cube("sword blade", (x, -3.5, 43.0), (2.0, 0.7, 7.0))
            cube("sword guard", (x, -3.5, 36.5), (4.5, 1.0, 0.9))
        else:
            cube("axe head", (x + 2.0, -3.5, 40.0), (4.5, 0.9, 4.0))
    export(output_root, "furniture/weapons-rack.glb")


def build_door_stand(output_root: Path) -> None:
    """Build the low plastic clip used by the original cardboard doors."""

    def export_stand(relative: str) -> None:
        clear_scene()
        plinth = cube("door stand plinth", (0.0, 0.0, 1.15), (18.0, 7.5, 1.15))
        bevel_object(plinth, 1.1, 4)
        for x in (-14.3, 14.3):
            clamp = cube("cardboard end clamp", (x, 0.0, 4.0), (2.7, 5.6, 4.0))
            bevel_object(clamp, 0.8, 3)
        for y in (-1.25, 1.25):
            lip = cube("cardboard slot lip", (0.0, y, 3.0), (13.0, 0.55, 2.0))
            bevel_object(lip, 0.35, 3)
        export(output_root, relative)

    # The open and closed pieces use the same physical stand but remain
    # separate runtime slots because their cardboard inserts differ.
    export_stand("doors/open.glb")
    export_stand("doors/closed.glb")


def quarter_ring_prism(
    name: str,
    pivot: tuple[float, float],
    inner_radius: float,
    outer_radius: float,
    height: float,
    segments: int = 20,
):
    """Create one solid quarter-annular stair tread on the XY floor plane."""

    vertices: list[tuple[float, float, float]] = []
    for z in (0.0, height):
        for index in range(segments + 1):
            angle = math.pi * 0.5 * index / segments
            cosine = math.cos(angle)
            sine = math.sin(angle)
            vertices.extend(
                (
                    (
                        pivot[0] + inner_radius * cosine,
                        pivot[1] + inner_radius * sine,
                        z,
                    ),
                    (
                        pivot[0] + outer_radius * cosine,
                        pivot[1] + outer_radius * sine,
                        z,
                    ),
                )
            )

    row = 2 * (segments + 1)
    faces: list[tuple[int, ...]] = []
    for index in range(segments):
        lower_inner = 2 * index
        lower_outer = lower_inner + 1
        next_lower_inner = lower_inner + 2
        next_lower_outer = lower_outer + 2
        upper_inner = row + lower_inner
        upper_outer = row + lower_outer
        next_upper_inner = row + next_lower_inner
        next_upper_outer = row + next_lower_outer
        faces.extend(
            (
                (lower_inner, next_lower_inner, next_lower_outer, lower_outer),
                (upper_inner, upper_outer, next_upper_outer, next_upper_inner),
                (lower_inner, upper_inner, next_upper_inner, next_lower_inner),
                (lower_outer, next_lower_outer, next_upper_outer, upper_outer),
            )
        )
    faces.extend(
        (
            (0, 1, row + 1, row),
            (2 * segments, row + 2 * segments, row + 2 * segments + 1, 2 * segments + 1),
        )
    )

    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return obj


def build_stair_tile(output_root: Path) -> None:
    """Build the fan-shaped 2x2 stairway symbol used by the US Quest Book."""

    clear_scene()
    cube("thin 2x2 stairway base", (0.0, 0.0, 0.45), (22.0, 22.0, 0.45))
    pivot = (-20.0, -20.0)
    tread_count = 10
    inner_start = 2.0
    tread_width = 3.8
    for index in range(tread_count):
        tread = quarter_ring_prism(
            f"curved stair tread {index + 1}",
            pivot,
            inner_start + index * tread_width,
            inner_start + (index + 1) * tread_width,
            1.15 + index * 0.45,
        )
        bevel_object(tread, 0.18, 2)

    cylinder("stairway newel", (pivot[0], pivot[1], 2.8), 2.3, 5.6, vertices=28)
    cube("lower curved-stair curb", (0.0, -20.0, 2.1), (20.0, 0.65, 2.1))
    cube("side curved-stair curb", (-20.0, 0.0, 2.1), (0.65, 20.0, 2.1))
    export(output_root, "markers/stairs.glb")


def add_rocks(width: float) -> None:
    cube("cardboard marker", (0.0, 0.0, 1.0), (width, 11.0, 1.0))
    columns = max(3, round(width / 5.0))
    for index in range(columns):
        x = -width + 3.5 + index * (2.0 * width - 7.0) / max(1, columns - 1)
        y = (-1.0 if index % 2 else 1.0) * (2.0 + (index % 3))
        bpy.ops.mesh.primitive_ico_sphere_add(
            subdivisions=1,
            radius=3.5 + (index % 3) * 0.7,
            location=(x, y, 4.2),
        )


def build_blocked_tiles(output_root: Path) -> None:
    clear_scene()
    add_rocks(11.0)
    export(output_root, "markers/blocked-square-1x1.glb")
    clear_scene()
    add_rocks(22.0)
    export(output_root, "markers/blocked-square-1x2.glb")


def build_skull_tile(output_root: Path) -> None:
    clear_scene()
    cube("cardboard skull marker", (0.0, 0.0, 0.7), (11.0, 11.0, 0.7))
    cylinder("cranium", (0.0, -1.5, 2.2), 6.0, 1.2)
    cube("jaw", (0.0, 4.0, 2.2), (3.8, 3.5, 0.6))
    for x in (-2.2, 2.2):
        cylinder("eye socket", (x, -2.0, 2.9), 1.25, 0.5)
    for x in (-2.4, -0.8, 0.8, 2.4):
        cube("tooth", (x, 6.4, 2.9), (0.55, 1.1, 0.3))
    export(output_root, "markers/skull.glb")


def build_furniture_rat(output_root: Path) -> None:
    """Reconstruct the crouched rat fitting shown on original-US furniture.

    The physical pieces are tiny single-color injection-molded decorations,
    so recognizable silhouette, molded fur, ears, paws, and the raised tail
    matter more at board distance than hidden underside detail.
    """

    clear_scene()
    body = uv_sphere("crouched body", (0.0, 0.0, 4.9), (7.0, 3.65, 3.75), 48, 28)
    head = uv_sphere("pointed head", (-5.9, 0.0, 5.0), (3.45, 2.65, 2.85), 42, 24)
    cone_between(
        "tapered muzzle",
        (-7.0, 0.0, 4.85),
        (-11.3, 0.0, 4.35),
        2.15,
        0.58,
        32,
    )
    uv_sphere("nose", (-11.55, 0.0, 4.30), (0.62, 0.60, 0.57), 24, 16)

    # A very restrained displacement reads as the coarse molded fur visible in
    # photographs without turning a 12 mm accessory into a noisy rock.
    fur = bpy.data.textures.new("fine molded fur", type="CLOUDS")
    fur.noise_scale = 0.65
    fur.noise_depth = 1
    for obj in (body, head):
        modifier = obj.modifiers.new(name="molded fur", type="DISPLACE")
        modifier.texture = fur
        modifier.strength = 0.22
        modifier.mid_level = 0.50
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.modifier_apply(modifier=modifier.name)

    for side in (-1.0, 1.0):
        uv_sphere(
            "rounded ear",
            (-6.05, side * 2.15, 7.35),
            (1.05, 0.45, 1.18),
            30,
            18,
        )
        uv_sphere(
            "eye",
            (-7.55, side * 2.12, 5.85),
            (0.38, 0.30, 0.38),
            20,
            12,
        )
        uv_sphere(
            "rear haunch",
            (3.65, side * 2.05, 3.45),
            (2.75, 1.45, 2.15),
            30,
            18,
        )
        uv_sphere(
            "rear paw",
            (5.15, side * 2.35, 1.55),
            (1.65, 0.74, 0.46),
            28,
            16,
        )
        uv_sphere(
            "front paw",
            (-6.95, side * 2.05, 1.45),
            (1.55, 0.62, 0.42),
            28,
            16,
        )
        for whisker_index, rise in enumerate((-0.55, 0.0, 0.55)):
            cylinder_between(
                "whisker",
                (-9.7, side * 1.25, 4.65 + rise),
                (-12.9 + whisker_index * 0.20, side * (2.55 + whisker_index * 0.24), 4.55 + rise),
                0.065,
                8,
            )

    tail_points = (
        (5.35, 0.0, 4.9),
        (7.8, 0.3, 4.1),
        (10.1, 0.72, 4.55),
        (11.7, 1.0, 5.9),
        (12.1, 0.9, 7.6),
        (11.6, 0.65, 8.9),
        (10.7, 0.3, 9.8),
    )
    for index, (start, end) in enumerate(zip(tail_points, tail_points[1:])):
        cylinder_between(
            "raised tapering tail",
            start,
            end,
            0.48 - index * 0.045,
            14,
        )
    cylinder("furniture peg", (0.5, 0.0, 0.35), 1.25, 1.6, vertices=20)
    export(output_root, "dressing/rat.glb")


def build_furniture_skull(output_root: Path) -> None:
    """Build the small ivory skull fitting supplied with classic furniture."""

    clear_scene()
    cranium = uv_sphere("cranium", (0.0, 0.0, 6.0), (4.35, 3.65, 4.45), 56, 32)
    for side in (-1.0, 1.0):
        cutter = uv_sphere(
            "eye socket cutter",
            (side * 1.62, -3.0, 6.25),
            (1.25, 1.55, 1.55),
            32,
            20,
        )
        subtract(cranium, cutter)
    nose = uv_sphere("nasal cavity cutter", (0.0, -3.45, 4.55), (0.8, 1.1, 1.15), 28, 18)
    subtract(cranium, nose)

    jaw = cube("lower jaw", (0.0, -0.4, 2.35), (2.9, 2.7, 1.55))
    bevel_object(jaw, 0.75, 4)
    for side in (-1.0, 1.0):
        cheek = uv_sphere(
            "cheek bone",
            (side * 2.8, -1.9, 4.25),
            (1.2, 1.35, 1.35),
            30,
            18,
        )
        cheek.scale.z = 0.7
    for index, x in enumerate((-1.8, -0.9, 0.0, 0.9, 1.8)):
        tooth = cube("individual tooth", (x, -3.1, 2.95), (0.34, 0.42, 0.82))
        bevel_object(tooth, 0.12, 2)
        if index in (0, 4):
            tooth.scale.z = 0.9
    cylinder("furniture peg", (0.0, 0.0, 0.25), 1.15, 1.5, vertices=20)
    export(output_root, "dressing/skull.glb")


PIPS = {
    1: ((0.0, 0.0),),
    2: ((-3.6, -3.6), (3.6, 3.6)),
    3: ((-4.0, -4.0), (0.0, 0.0), (4.0, 4.0)),
    4: ((-3.8, -3.8), (3.8, -3.8), (-3.8, 3.8), (3.8, 3.8)),
    5: ((-4.0, -4.0), (4.0, -4.0), (0.0, 0.0), (-4.0, 4.0), (4.0, 4.0)),
    6: ((-3.8, -4.5), (3.8, -4.5), (-3.8, 0.0), (3.8, 0.0), (-3.8, 4.5), (3.8, 4.5)),
}


def add_die_body() -> None:
    body = cube("rounded die", (0.0, 0.0, 0.0), (8.0, 8.0, 8.0))
    bevel = body.modifiers.new(name="rounded corners", type="BEVEL")
    bevel.width = 1.15
    bevel.segments = 3
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.modifier_apply(modifier=bevel.name)


def add_movement_pips() -> None:
    # Face numbering matches src/dice.rs: +Y, +Z, +X, -X, -Z, -Y.
    faces = (
        (1, (0.0, 8.05, 0.0), (math.pi / 2.0, 0.0, 0.0), "y", 1.0),
        (2, (0.0, 0.0, 8.05), (0.0, 0.0, 0.0), "z", 1.0),
        (3, (8.05, 0.0, 0.0), (0.0, math.pi / 2.0, 0.0), "x", 1.0),
        (4, (-8.05, 0.0, 0.0), (0.0, math.pi / 2.0, 0.0), "x", -1.0),
        (5, (0.0, 0.0, -8.05), (0.0, 0.0, 0.0), "z", -1.0),
        (6, (0.0, -8.05, 0.0), (math.pi / 2.0, 0.0, 0.0), "y", -1.0),
    )
    for face, origin, rotation, axis, sign in faces:
        for a, b in PIPS[face]:
            if axis == "y":
                location = (a, origin[1], b)
            elif axis == "x":
                location = (origin[0], a, b)
            else:
                location = (a, b, origin[2])
            cylinder("pip", location, 1.05, 0.45, rotation)


def build_dice(output_root: Path) -> None:
    clear_scene()
    add_die_body()
    add_movement_pips()
    export(output_root, "dice/movement.glb")
    reference = output_root / "dice/combat-reference.glb"
    clear_scene()
    if reference.is_file():
        bpy.ops.import_scene.gltf(filepath=str(reference))
    else:
        add_die_body()
    # Renderer physics animates this mesh and overlays the scan-derived face
    # decals; keep the printable body geometry in the private asset pack.
    export(output_root, "dice/combat.glb")


def main() -> None:
    output_root = arguments().output_root
    build_weapon_rack(output_root)
    build_door_stand(output_root)
    # The original stairway is a flat scan-backed punchboard cutout, so the
    # runtime deliberately does not build or use a printable relief model.
    build_blocked_tiles(output_root)
    build_skull_tile(output_root)
    build_furniture_rat(output_root)
    build_furniture_skull(output_root)
    build_dice(output_root)
    print("Built project-authored door stands, weapon rack, markers, furniture dressing, and dice assets")


if __name__ == "__main__":
    main()
