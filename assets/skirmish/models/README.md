# Skirmish models

Light models for the generic skirmish's unit kinds, drawn by `render3d` (docs/render.md) in place of boxes and built
into the program. `models.json` lists them by kind and says which generic studio model each comes from.

They are made with the art studio in the Classic engine's repository (rts-engine), from its generic placeholder
models, with Blender's Python module (`pip install bpy`, version 5.2.2) and the studio's exporter at low detail:

```bash
python3 art/studio/export_gltf.py vehicles/mcv buildings/power_plant buildings/silo buildings/heavy_factory \
    vehicles/battle_tank vehicles/missile_tank --out OUT --lod low
```

and each `OUT/<id>-low.glb` is copied here under its kind's name. They hold no setting's names or art.
