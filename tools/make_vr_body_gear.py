"""Rebuild modular original-textured VR gear with Blender, without a UI or MCP.

/Applications/Blender.app/Contents/MacOS/blender --background --python tools/make_vr_body_gear.py

Exports metre-scale, Y-up GLBs with direct astra-vr-body-gear-color.png references.
The editable source scene stays in assets/source/astra-vr-body-gear.blend.
"""
import bpy
import json
import math
import struct
import tempfile
from pathlib import Path
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets"
PREVIEWS = Path("/tmp/astra-body-gear")
PREVIEWS.mkdir(parents=True, exist_ok=True)
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)
image = bpy.data.images.load(str(ASSETS / "astra-vr-body-gear-color.png"), check_existing=True)


def material(name, patch, roughness):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    shader = mat.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Roughness"].default_value = roughness
    tex = mat.node_tree.nodes.new("ShaderNodeTexImage")
    tex.image = image
    mat.node_tree.links.new(tex.outputs["Color"], shader.inputs["Base Color"])
    mat["atlas_patch"] = patch
    return mat


webbing = material("Gear | dark webbing", [.02, .02, .48, .48], .92)
cloth = material("Gear | blue-grey fabric", [.52, .52, .98, .98], .85)
rubber = material("Gear | rubber edging", [.52, .02, .98, .48], .72)
polymer = material("Gear | charcoal molded polymer", [.02, .52, .48, .98], .48)
metal = material("Gear | blue-grey hardware", [.02, .52, .48, .98], .4)


def finish(obj, mat, bevel=.001):
    obj.data.materials.append(mat)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    # One flat, 1 mm chamfer catches highlights without rounding the silhouette.
    if bevel > 0:
        modifier = obj.modifiers.new("Small edge chamfer", "BEVEL")
        modifier.width = bevel
        modifier.segments = 1
        modifier.affect = "EDGES"
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(island_margin=.02)
    bpy.ops.object.mode_set(mode="OBJECT")
    x0, y0, x1, y1 = mat["atlas_patch"]
    for uv in obj.data.uv_layers.active.data:
        u, v = uv.uv
        uv.uv = (x0 + u * (x1-x0), 1-y1 + v * (y1-y0))
    return obj


def box(name, position, size, mat, bevel=.001):
    bpy.ops.mesh.primitive_cube_add(size=1, location=position)
    obj = bpy.context.object
    obj.name = name
    obj.dimensions = size
    return finish(obj, mat, bevel)



parts={}
start=set(bpy.data.objects)
# Front strap only: no back or side loop around an inferred torso.
# A shallow front arc, stopping well before either hip. Segment the strap
# before bending so its broad faces follow the waist rather than stay planar.
strap = box("Belt front strap", (0, .30, 0), (.42, .018, .055), webbing, 0)
bpy.context.view_layer.objects.active = strap
bpy.ops.object.mode_set(mode="EDIT")
bpy.ops.mesh.select_all(action="SELECT")
bpy.ops.mesh.subdivide(number_cuts=11)
bpy.ops.object.mode_set(mode="OBJECT")
for x in [-.19,.19]: box("Belt end block",(x,.315,0),(.026,.020,.063),rubber)
box("Belt flat buckle",(-.115,.316,0),(.055,.012,.065),metal)
box("Belt buckle inset",(-.115,.324,0),(.034,.005,.041),webbing)
parts["belt"]=set(bpy.data.objects)-start
# Bend the strap and its attached hardware together in the horizontal plane.
# 50 cm radius gives ~4 cm of sweep over the 42 cm front piece.
belt_radius = .50
for obj in parts["belt"]:
    for vertex in obj.data.vertices:
        point = obj.matrix_world @ vertex.co
        angle = point.x / belt_radius
        radius = belt_radius + point.y - .30
        point.x = radius * math.sin(angle)
        point.y = .30 + radius * math.cos(angle) - belt_radius
        vertex.co = obj.matrix_world.inverted() @ point
# Chamfer after bending so the continuous strap has the same small edge finish.
bpy.context.view_layer.objects.active = strap
modifier = strap.modifiers.new("Small edge chamfer", "BEVEL")
modifier.width = .001
modifier.segments = 1
modifier.limit_method = "ANGLE"
bpy.ops.object.modifier_apply(modifier=modifier.name)

start=set(bpy.data.objects)
# Five plain rectangular panels make the opening and depth unambiguous.
box("Pouch back",(0,-.03,0),(.15,.014,.17),cloth)
box("Pouch front",(0,.035,-.018),(.15,.014,.134),cloth)
for x in [-.070,.070]: box("Pouch side",(x,0,-.015),(.015,.06,.14),rubber)
box("Pouch bottom",(0,0,-.080),(.15,.075,.016),rubber)
box("Pouch mounting block",(0,-.044,.035),(.06,.016,.065),webbing)
parts["ammo-pouch"]=set(bpy.data.objects)-start

start=set(bpy.data.objects)
# Rigid open cradle: tapered spine, swept side cheeks and a short retention
# bridge leave the weapon exposed rather than enclosing it in a fabric pocket.
spine = box("Holster polymer spine", (0, -.026, .017), (.10, .014, .24), polymer)
for vertex in spine.data.vertices:
    vertex.co.x *= .78 + .22 * ((vertex.co.z + .12) / .24)
box("Holster mounting rail", (0, -.040, .025), (.036, .016, .15), rubber)
# Extruded angular profiles in Y/Z; the top opening flares toward the wearer.
profile = [(-.027, -.095), (.022, -.095), (.034, -.052),
           (.034, .016), (.006, .066), (-.027, .084)]
for side in [-1, 1]:
    vertices = [(side * .047 + dx, y, z) for dx in [-.007, .007] for y, z in profile]
    count = len(profile)
    faces = [tuple(reversed(range(count))), tuple(range(count, 2 * count))]
    faces += [(i, (i + 1) % count, (i + 1) % count + count, i + count) for i in range(count)]
    mesh = bpy.data.meshes.new("Molded holster cheek")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new("Holster angular cheek", mesh)
    bpy.context.collection.objects.link(obj)
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    finish(obj, polymer, .0015)
box("Holster retention bridge", (0, .032, -.057), (.10, .014, .022), polymer)
box("Holster lower stop", (0, -.002, -.095), (.09, .054, .016), polymer)
# Dark inset reserves a clearly separated surface for a future ammo indicator.
box("Holster indicator recess", (0, .040, -.057), (.049, .003, .008), rubber, .0005)
parts["holster"]=set(bpy.data.objects)-start


def export_glb(name, objects):
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects: obj.select_set(True)
    with tempfile.TemporaryDirectory(prefix="astra-gear-export-") as tmp:
        path=Path(tmp)/"model.gltf"
        bpy.ops.export_scene.gltf(filepath=str(path),export_format="GLTF_SEPARATE",use_selection=True,
            export_yup=True,export_apply=True,export_texcoords=True,export_normals=True,
            export_animations=False,export_cameras=False,export_lights=False)
        doc=json.loads(path.read_text())
        binary=(Path(tmp)/doc["buffers"][0]["uri"]).read_bytes()
        doc["buffers"][0].pop("uri")
        for texture in doc.get("images",[]): texture["uri"]="astra-vr-body-gear-color.png"
        encoded=json.dumps(doc,separators=(",",":")).encode()
        encoded += b" "*((-len(encoded))%4)
        binary += b"\0"*((-len(binary))%4)
        glb=struct.pack("<III",0x46546c67,2,12+8+len(encoded)+8+len(binary))
        glb+=struct.pack("<II",len(encoded),0x4e4f534a)+encoded
        glb+=struct.pack("<II",len(binary),0x004e4942)+binary
        out=ASSETS/f"astra-vr-{name}.glb"
        for obj in objects: obj.data.calc_loop_triangles()
        out.write_bytes(glb)
        print(f"GEAR_EXPORT {out.name} bytes={len(glb)} triangles={sum(len(o.data.loop_triangles) for o in objects)}")


for name,objects in parts.items(): export_glb(name,objects)
# Keep all modular parts editable in one source scene, spaced for inspection.
for name,offset in [("belt",(-.28,0,.10)),("ammo-pouch",(.19,.03,.10)),("holster",(.43,.03,.10))]:
    root=bpy.data.objects.new(name.upper(),None)
    bpy.context.collection.objects.link(root)
    for obj in parts[name]: obj.parent=root
    root.location=offset

scene=bpy.context.scene
scene.render.engine="CYCLES"
scene.cycles.samples=32
scene.render.resolution_x=1400
scene.render.resolution_y=820
scene.render.resolution_percentage=100
scene.world.color=(.12,.12,.12)
bpy.ops.mesh.primitive_plane_add(size=200, location=(0,0,-.031))
floor=bpy.context.object; floor.name="Preview floor (not exported)"
mat=bpy.data.materials.new("Preview neutral floor");mat.diffuse_color=(.12,.14,.17,1);floor.data.materials.append(mat)
for name,loc,power,size in [("Key",(-1,2,2),220,3),("Fill",(1,1,1),130,2),("Rim",(0,-1,2),270,2)]:
    bpy.ops.object.light_add(type="AREA",location=loc)
    lamp=bpy.context.object;lamp.name=name;lamp.data.energy=power;lamp.data.shape="DISK";lamp.data.size=size
    lamp.rotation_euler=(Vector((0,0,.08))-lamp.location).to_track_quat('-Z','Y').to_euler()
bpy.ops.object.camera_add(location=(.75,1.35,.80))
camera=bpy.context.object;camera.name="Preview camera";camera.data.type="ORTHO";camera.data.ortho_scale=1.20
camera.rotation_euler=(Vector((.0,0,.055))-camera.location).to_track_quat('-Z','Y').to_euler()
scene.camera=camera
scene.render.image_settings.file_format="PNG"
for name,location in [("front",(.75,1.35,.80)),("back",(-.6,-1.35,.8))]:
    camera.location=location
    camera.rotation_euler=(Vector((0,0,.055))-camera.location).to_track_quat('-Z','Y').to_euler()
    scene.render.filepath=str(PREVIEWS/f"{name}.png")
    bpy.ops.render.render(write_still=True)
source=ASSETS/"source"/"astra-vr-body-gear.blend"
source.parent.mkdir(exist_ok=True)
bpy.context.preferences.filepaths.save_version=0
image.filepath="//../astra-vr-body-gear-color.png"
bpy.ops.wm.save_as_mainfile(filepath=str(source))
