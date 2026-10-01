import AppKit
import SceneKit
import SwiftUI
import WaveyCore

/// 3D view of the extruded room. The model is z-up; SceneKit is y-up, so all
/// room content lives under `modelRoot`, which rotates z onto y.
struct RoomViewport: NSViewRepresentable {
    var model: AppModel
    /// Called with the surface key under a click (nil for empty space).
    var onPickSurface: ((String?) -> Void)?
    /// Extra content (speakers, mics, cut planes) added under the model root.
    var decorate: ((SCNNode) -> Void)?
    var decorationRevision: Int = 0

    func makeCoordinator() -> ViewportCoordinator { ViewportCoordinator() }

    func makeNSView(context: Context) -> SCNView {
        let view = SCNView()
        view.scene = context.coordinator.scene
        view.allowsCameraControl = true
        view.defaultCameraController.interactionMode = .orbitTurntable
        view.defaultCameraController.inertiaEnabled = true
        view.backgroundColor = .windowBackgroundColor
        view.antialiasingMode = .multisampling4X
        view.pointOfView = context.coordinator.cameraNode
        let click = NSClickGestureRecognizer(target: context.coordinator, action: #selector(ViewportCoordinator.clicked(_:)))
        view.addGestureRecognizer(click)
        context.coordinator.view = view
        return view
    }

    func updateNSView(_ view: SCNView, context: Context) {
        let c = context.coordinator
        c.onPickSurface = onPickSurface
        c.sync(room: model.room, revision: model.revision, selected: model.selectedSurface,
               surfaceColor: { key in model.surfaceColor(key) })
        if c.decorationRevision != decorationRevision || c.decoratedRoomRevision != model.revision {
            c.decorationRevision = decorationRevision
            c.decoratedRoomRevision = model.revision
            c.decorationsNode.childNodes.forEach { $0.removeFromParentNode() }
            decorate?(c.decorationsNode)
        }
    }
}

@MainActor
final class ViewportCoordinator: NSObject {
    let scene = SCNScene()
    let modelRoot = SCNNode()
    let surfacesNode = SCNNode()
    let decorationsNode = SCNNode()
    let cameraNode = SCNNode()
    weak var view: SCNView?
    var onPickSurface: ((String?) -> Void)?
    var decorationRevision = -1
    var decoratedRoomRevision = -1

    private var builtRevision = -1
    private var framedOnce = false
    private var surfaceNodes: [String: SCNNode] = [:]

    override init() {
        super.init()
        modelRoot.eulerAngles.x = -.pi / 2
        modelRoot.addChildNode(surfacesNode)
        modelRoot.addChildNode(decorationsNode)
        scene.rootNode.addChildNode(modelRoot)

        let camera = SCNCamera()
        camera.zNear = 0.05
        camera.zFar = 2000
        camera.fieldOfView = 45
        cameraNode.camera = camera
        cameraNode.position = SCNVector3(0, 15, 25)
        scene.rootNode.addChildNode(cameraNode)

        let ambient = SCNNode()
        ambient.light = SCNLight()
        ambient.light!.type = .ambient
        ambient.light!.intensity = 300
        scene.rootNode.addChildNode(ambient)
        let sun = SCNNode()
        sun.light = SCNLight()
        sun.light!.type = .directional
        sun.light!.intensity = 900
        sun.eulerAngles = SCNVector3(-0.9, 0.5, 0)
        scene.rootNode.addChildNode(sun)
        // A weaker fill from the opposite side keeps walls facing away from the sun distinguishable.
        let fill = SCNNode()
        fill.light = SCNLight()
        fill.light!.type = .directional
        fill.light!.intensity = 300
        fill.eulerAngles = SCNVector3(-0.4, 0.5 + .pi, 0)
        scene.rootNode.addChildNode(fill)
    }

    /// Converts a model-space point (z up) to scene space.
    static func scenePoint(_ x: Double, _ y: Double, _ z: Double) -> SCNVector3 {
        SCNVector3(x, z, -y)
    }

    func sync(room: RoomSnapshot?, revision: Int, selected: String?, surfaceColor: (String) -> NSColor) {
        if revision != builtRevision {
            builtRevision = revision
            rebuild(room)
        }
        for (key, node) in surfaceNodes {
            let m = node.geometry?.firstMaterial
            m?.diffuse.contents = key == selected ? NSColor.systemOrange : surfaceColor(key)
            m?.emission.contents = key == selected ? NSColor.systemOrange.withAlphaComponent(0.3) : NSColor.black
        }
    }

    private func rebuild(_ room: RoomSnapshot?) {
        surfacesNode.childNodes.forEach { $0.removeFromParentNode() }
        surfaceNodes = [:]
        guard let room else { return }
        for s in room.surfaces {
            var verts: [SCNVector3] = []
            verts.reserveCapacity(s.positions.count / 3)
            for i in stride(from: 0, to: s.positions.count, by: 3) {
                verts.append(SCNVector3(s.positions[i], s.positions[i + 1], s.positions[i + 2]))
            }
            let n = SCNVector3(s.normal.x, s.normal.y, s.normal.z)
            let geom = SCNGeometry(
                sources: [SCNGeometrySource(vertices: verts), SCNGeometrySource(normals: Array(repeating: n, count: verts.count))],
                elements: [SCNGeometryElement(indices: s.indices, primitiveType: .triangles)])
            let mat = SCNMaterial()
            mat.lightingModel = .lambert
            mat.isDoubleSided = false
            geom.materials = [mat]
            let node = SCNNode(geometry: geom)
            node.name = s.key
            surfacesNode.addChildNode(node)
            surfaceNodes[s.key] = node
        }
        if !framedOnce {
            framedOnce = true
            frame(room)
        }
    }

    /// Points the camera at the room from a raised three-quarter view.
    func frame(_ room: RoomSnapshot) {
        let lo = room.boundsMin, hi = room.boundsMax
        let c = Self.scenePoint((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, (lo.z + hi.z) / 2)
        let size = max(hi.x - lo.x, hi.y - lo.y, hi.z - lo.z)
        cameraNode.position = SCNVector3(c.x + size * 0.6, c.y + size * 0.9, c.z + size * 1.1)
        cameraNode.look(at: c)
        view?.defaultCameraController.target = c
    }

    @objc func clicked(_ g: NSClickGestureRecognizer) {
        guard let view else { return }
        let p = g.location(in: view)
        let hits = view.hitTest(p, options: [
            .backFaceCulling: true,
            .searchMode: SCNHitTestSearchMode.closest.rawValue,
            .rootNode: surfacesNode,
        ])
        onPickSurface?(hits.first?.node.name)
    }
}
