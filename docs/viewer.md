# Mesh viewer

`fh view MODEL.inp` opens a window with the mesh of a model in 3D. It shows no results yet (D-20).

```
fh mesh part.step -o part.msh --order 2   # a mesh from gmsh
# write or convert the mesh to .inp, then:
fh view model.inp
```

The viewer reads the `.inp` file with the `inp` crate. It draws `C3D4`, `C3D8` (also `C3D8R` and `C3D8I`) and `C3D10` elements. It skips other element types and elements that name a missing node. The status bar shows the count.

What you see:

- The surface of the part (the faces that belong to one element only).
- The element edges on the surface (grey).
- The outline of the part (dark lines): the edges where the surface turns by more than 30 degrees, and the edges at an open surface.
- The number of nodes and elements.

Controls:

| Action | Input |
|---|---|
| Orbit | drag with the left button |
| Pan | drag with the right or middle button, or shift and left button |
| Zoom | scroll |
| Reset the view | double click |

The window needs a GPU with Metal, Vulkan or DX12. The viewer is native only. The wasm build is not started.

The code is in `crates/viewer`. `mesh.rs` (surface, outline, bounding box) and `camera.rs` have no GPU types and have unit tests. `gpu.rs` and `app.rs` are the thin window layer.
