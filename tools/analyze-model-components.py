#!/usr/bin/env python3
"""Print disconnected mesh-island bounds for a GLB, OBJ, or STL."""

from __future__ import annotations

import sys
from collections import deque
from pathlib import Path

import bpy


def mesh_components(obj: bpy.types.Object) -> list[tuple[list[int], tuple[float, ...]]]:
    vertices = obj.data.vertices
    adjacency: list[list[int]] = [[] for _ in vertices]
    for edge in obj.data.edges:
        a, b = edge.vertices
        adjacency[a].append(b)
        adjacency[b].append(a)

    unseen = set(range(len(vertices)))
    components = []
    while unseen:
        seed = unseen.pop()
        queue = deque((seed,))
        indices = [seed]
        while queue:
            current = queue.popleft()
            for linked in adjacency[current]:
                if linked in unseen:
                    unseen.remove(linked)
                    queue.append(linked)
                    indices.append(linked)
        coordinates = [obj.matrix_world @ vertices[index].co for index in indices]
        bounds = (
            min(co.x for co in coordinates),
            max(co.x for co in coordinates),
            min(co.y for co in coordinates),
            max(co.y for co in coordinates),
            min(co.z for co in coordinates),
            max(co.z for co in coordinates),
        )
        components.append((indices, bounds))
    return sorted(components, key=lambda component: len(component[0]), reverse=True)


def main() -> None:
    path = Path(sys.argv[sys.argv.index("--") + 1])
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    match path.suffix.lower():
        case ".glb" | ".gltf":
            bpy.ops.import_scene.gltf(filepath=str(path))
        case ".obj":
            if hasattr(bpy.ops.wm, "obj_import"):
                bpy.ops.wm.obj_import(filepath=str(path))
            else:
                bpy.ops.import_scene.obj(filepath=str(path))
        case ".stl":
            if hasattr(bpy.ops.wm, "stl_import"):
                bpy.ops.wm.stl_import(filepath=str(path))
            else:
                bpy.ops.import_mesh.stl(filepath=str(path))
        case suffix:
            raise RuntimeError(f"unsupported model extension: {suffix}")
    print(f"MODEL {path}")
    for obj in bpy.context.scene.objects:
        if obj.type != "MESH":
            continue
        print(
            f"OBJECT {obj.name} vertices={len(obj.data.vertices)} "
            f"polygons={len(obj.data.polygons)}"
        )
        for number, (indices, bounds) in enumerate(mesh_components(obj), start=1):
            rounded = tuple(round(value, 3) for value in bounds)
            print(f"  ISLAND {number} vertices={len(indices)} bounds={rounded}")


if __name__ == "__main__":
    main()
