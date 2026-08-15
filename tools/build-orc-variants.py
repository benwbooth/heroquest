#!/usr/bin/env python3
"""Build the two quest-specific Orc variants from the classic sword Orc scan.

The original-US box contains a distinct enlarged/notched-sword Orc, but no
separate staff Orc.  The notched variant retains the scanned figure and swaps
only its blade for the photographed original silhouette.  Grak retains the
same scanned figure while receiving a project-authored staff, as directed by
Quest 6.

Run through Blender, for example:
  blender --background --python tools/build-orc-variants.py -- \
    --source assets/local/editions/original-us/models/figures/orc-sword.glb \
    --output-root assets/local/editions/original-us/models
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import bpy
import bmesh


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    return parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])


def clear_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for blocks in (bpy.data.meshes, bpy.data.curves, bpy.data.materials):
        for block in list(blocks):
            if block.users == 0:
                blocks.remove(block)


def import_figure(source: Path) -> bpy.types.Object:
    if not source.is_file():
        raise FileNotFoundError(source)
    bpy.ops.import_scene.gltf(filepath=str(source))
    meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    if not meshes:
        raise RuntimeError(f"no mesh found in {source}")
    figure = max(meshes, key=lambda obj: len(obj.data.vertices))
    figure.name = "Classic scanned Orc"
    return figure


def cube_cutter(
    name: str,
    location: tuple[float, float, float],
    half_extents: tuple[float, float, float],
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cube_add(location=location)
    cutter = bpy.context.object
    cutter.name = name
    cutter.scale = half_extents
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return cutter


def remove_original_blade(figure: bpy.types.Object) -> None:
    # The blade occupies this isolated volume in the raised-hand scan.  The
    # exact boolean seals the cut while preserving the fist, arm, and base.
    cutter = cube_cutter("temporary blade cutter", (-8.0, 16.3, 36.7), (10.3, 4.1, 4.2))
    modifier = figure.modifiers.new(name="Remove original sword blade", type="BOOLEAN")
    modifier.operation = "DIFFERENCE"
    modifier.solver = "EXACT"
    modifier.object = cutter
    bpy.context.view_layer.objects.active = figure
    bpy.ops.object.modifier_apply(modifier=modifier.name)
    bpy.data.objects.remove(cutter, do_unlink=True)


def subtract_cutter(figure: bpy.types.Object, cutter: bpy.types.Object, name: str) -> None:
    modifier = figure.modifiers.new(name=name, type="BOOLEAN")
    modifier.operation = "DIFFERENCE"
    modifier.solver = "EXACT"
    modifier.object = cutter
    bpy.context.view_layer.objects.active = figure
    bpy.ops.object.modifier_apply(modifier=modifier.name)
    bpy.data.objects.remove(cutter, do_unlink=True)


def remove_original_guard(figure: bpy.types.Object) -> None:
    # Grak carries a staff rather than a sword.  Two narrow cuts remove the
    # remaining quillons without touching the clenched hand to their right.
    upper = cube_cutter("temporary upper guard cutter", (1.5, 16.3, 36.25), (1.65, 4.0, 3.4))
    subtract_cutter(figure, upper, "Remove upper sword guard")
    lower = cube_cutter("temporary lower guard cutter", (-0.25, 16.3, 32.75), (3.35, 4.0, 2.15))
    subtract_cutter(figure, lower, "Remove lower sword guard")


def remove_small_mesh_islands(obj: bpy.types.Object, minimum_vertices: int = 500) -> None:
    """Discard weapon fragments detached by localized boolean cuts."""
    mesh = bmesh.new()
    mesh.from_mesh(obj.data)
    unseen = set(mesh.verts)
    discard = []
    while unseen:
        seed = unseen.pop()
        stack = [seed]
        island = [seed]
        while stack:
            current = stack.pop()
            for edge in current.link_edges:
                linked = edge.other_vert(current)
                if linked in unseen:
                    unseen.remove(linked)
                    stack.append(linked)
                    island.append(linked)
        if len(island) < minimum_vertices:
            discard.extend(island)
    if discard:
        bmesh.ops.delete(mesh, geom=discard, context="VERTS")
    mesh.to_mesh(obj.data)
    mesh.free()
    obj.data.update()


def prism(
    name: str,
    outline: tuple[tuple[float, float], ...],
    y_min: float,
    y_max: float,
) -> bpy.types.Object:
    count = len(outline)
    vertices = [(x, y_min, z) for x, z in outline] + [(x, y_max, z) for x, z in outline]
    faces: list[tuple[int, ...]] = []
    faces.append(tuple(reversed(range(count))))
    faces.append(tuple(range(count, count * 2)))
    for index in range(count):
        next_index = (index + 1) % count
        faces.append((index, next_index, count + next_index, count + index))
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return obj


def bevel_mesh(obj: bpy.types.Object, width: float) -> None:
    bevel = obj.modifiers.new(name="Moulded edge rounding", type="BEVEL")
    bevel.width = width
    bevel.segments = 2
    bevel.limit_method = "ANGLE"
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier=bevel.name)
    for polygon in obj.data.polygons:
        polygon.use_smooth = True


def build_notched_sword(source: Path, output_root: Path) -> None:
    clear_scene()
    figure = import_figure(source)
    remove_original_blade(figure)

    # Front-view outline measured from original-US miniature photographs.  The
    # lower-edge keyhole notch, straighter spine, heavier tip, and longer blade
    # distinguish the captain sculpt from the regular curved sword.
    outline = (
        (2.2, 34.1),
        (-4.2, 34.1),
        (-10.4, 34.2),
        (-10.5, 35.5),
        (-11.0, 36.25),
        (-11.8, 36.65),
        (-12.6, 36.25),
        (-13.05, 35.5),
        (-13.0, 34.15),
        (-16.2, 34.0),
        (-18.3, 35.0),
        (-18.8, 36.6),
        (-17.6, 38.0),
        (-14.6, 40.0),
        (-9.4, 40.8),
        (-3.6, 40.0),
        (2.2, 37.7),
    )
    blade = prism("Enlarged notched captain blade", outline, 15.25, 17.45)
    bevel_mesh(blade, 0.22)

    export(output_root / "figures/orc-notched-sword.glb")


def cylinder_between(
    name: str,
    start: tuple[float, float, float],
    end: tuple[float, float, float],
    radius: float,
    vertices: int = 16,
) -> bpy.types.Object:
    from mathutils import Vector

    start_vector = Vector(start)
    end_vector = Vector(end)
    direction = end_vector - start_vector
    midpoint = (start_vector + end_vector) * 0.5
    bpy.ops.mesh.primitive_cylinder_add(
        vertices=vertices,
        radius=radius,
        depth=direction.length,
        location=midpoint,
    )
    obj = bpy.context.object
    obj.name = name
    obj.rotation_euler = direction.to_track_quat("Z", "Y").to_euler()
    return obj


def sphere(name: str, location: tuple[float, float, float], radius: float) -> bpy.types.Object:
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=2, radius=radius, location=location)
    obj = bpy.context.object
    obj.name = name
    for polygon in obj.data.polygons:
        polygon.use_smooth = True
    return obj


def build_staff(source: Path, output_root: Path) -> None:
    clear_scene()
    figure = import_figure(source)
    remove_original_blade(figure)
    remove_original_guard(figure)
    remove_small_mesh_islands(figure)

    # A subtly crooked, knotted shaft follows the existing raised-fist grip.
    # Segment overlap is intentional and reads as carved knots after tinting.
    points = (
        (9.0, 16.3, 25.8),
        (6.2, 16.35, 30.2),
        (3.4, 16.35, 34.3),
        (-1.7, 16.25, 36.7),
        (-7.6, 16.15, 38.5),
        (-13.8, 16.0, 40.8),
    )
    for index, (start, end) in enumerate(zip(points, points[1:])):
        cylinder_between(f"Staff shaft {index + 1}", start, end, 0.72 + (index % 2) * 0.08)
    sphere("Staff lower ferrule", points[0], 1.0)
    sphere("Staff carved crown", points[-1], 1.55)
    sphere("Staff crown knot", (-12.8, 16.0, 42.1), 0.92)
    # Small bands around the grip and crown make the silhouette legible from
    # the board camera without pretending this was a separate box sculpt.
    for index, (start, end) in enumerate(
        (
            ((2.5, 16.35, 33.55), (4.1, 16.35, 35.05)),
            ((-12.4, 16.0, 40.25), (-14.1, 16.0, 41.2)),
        )
    ):
        cylinder_between(f"Staff binding {index + 1}", start, end, 1.02, vertices=18)
    export(output_root / "figures/orc-staff.glb")


def export(target: Path) -> None:
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
    print(f"Built {target}")


def main() -> None:
    args = arguments()
    build_notched_sword(args.source, args.output_root)
    build_staff(args.source, args.output_root)


if __name__ == "__main__":
    main()
