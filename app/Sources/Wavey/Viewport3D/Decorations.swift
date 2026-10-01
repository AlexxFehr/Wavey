import AppKit
import SceneKit
import WaveyCore

/// Builds SceneKit nodes for speakers, balloons, microphones and cut planes.
/// All nodes are created in model coordinates (z up).
@MainActor
enum Decorations {
    static func speakers(_ speakers: [SpeakerRec], models: [String: SpeakerModelRec], selected: Set<UInt64>,
                         balloon: (UInt64, BalloonMeshRec)?, into root: SCNNode) {
        for s in speakers {
            let m = models[s.modelId]
            let node = SCNNode()
            node.name = "spk:\(s.id)"
            node.simdTransform = transform(s)

            // Local frame: x on-axis (depth), y left (width), z up (height).
            let box = SCNBox(width: m?.depthM ?? 0.3, height: m?.widthM ?? 0.3, length: m?.heightM ?? 0.5, chamferRadius: 0.01)
            let isSelected = selected.contains(s.id)
            box.firstMaterial?.diffuse.contents = s.muted ? NSColor.darkGray
                : isSelected ? NSColor.systemOrange : NSColor(calibratedWhite: 0.15, alpha: 1)
            box.firstMaterial?.lightingModel = .lambert
            node.addChildNode(SCNNode(geometry: box))

            // Front baffle marker and aim arrow along +x.
            // SCNPlane faces +z; rotating +90° about y makes it face +x, with its
            // width then along z (speaker height) and height along y (speaker width).
            let front = SCNPlane(width: (m?.heightM ?? 0.5) * 0.8, height: (m?.widthM ?? 0.3) * 0.8)
            front.firstMaterial?.diffuse.contents = NSColor.systemBlue
            front.firstMaterial?.isDoubleSided = true
            let frontNode = SCNNode(geometry: front)
            frontNode.simdPosition = SIMD3(Float((m?.depthM ?? 0.3) / 2 + 0.002), 0, 0)
            frontNode.eulerAngles = SCNVector3(0, Double.pi / 2, 0)
            node.addChildNode(frontNode)
            node.addChildNode(arrow(length: 1.2, color: isSelected ? .systemOrange : .systemBlue))

            if let (id, mesh) = balloon, id == s.id {
                node.addChildNode(balloonNode(mesh, scale: 1.5))
            }
            root.addChildNode(node)
        }
    }

    static func transform(_ s: SpeakerRec) -> simd_float4x4 {
        let yaw = Float(s.yawDeg * .pi / 180), pitch = Float(s.pitchDeg * .pi / 180), roll = Float(s.rollDeg * .pi / 180)
        let rz = simd_quatf(angle: yaw, axis: [0, 0, 1])
        let ry = simd_quatf(angle: -pitch, axis: [0, 1, 0])
        let rx = simd_quatf(angle: roll, axis: [1, 0, 0])
        var t = simd_float4x4(rz * ry * rx)
        t.columns.3 = SIMD4(Float(s.position.x), Float(s.position.y), Float(s.position.z), 1)
        return t
    }

    static func arrow(length: Double, color: NSColor) -> SCNNode {
        let shaft = SCNCylinder(radius: 0.012, height: length)
        shaft.firstMaterial?.diffuse.contents = color
        let shaftNode = SCNNode(geometry: shaft)
        shaftNode.eulerAngles = SCNVector3(0, 0, -Double.pi / 2)  // cylinder y → x
        shaftNode.position = SCNVector3(length / 2, 0, 0)
        let head = SCNCone(topRadius: 0, bottomRadius: 0.04, height: 0.12)
        head.firstMaterial?.diffuse.contents = color
        let headNode = SCNNode(geometry: head)
        headNode.eulerAngles = SCNVector3(0, 0, -Double.pi / 2)
        headNode.position = SCNVector3(length, 0, 0)
        let n = SCNNode()
        n.addChildNode(shaftNode)
        n.addChildNode(headNode)
        return n
    }

    static func balloonNode(_ mesh: BalloonMeshRec, scale: Float) -> SCNNode {
        var verts: [SCNVector3] = []
        var colors: [Float] = []
        for i in 0..<(mesh.positions.count / 3) {
            verts.append(SCNVector3(mesh.positions[3 * i] * scale, mesh.positions[3 * i + 1] * scale, mesh.positions[3 * i + 2] * scale))
            let t = 1 - Double(mesh.levelDb[i]) / mesh.floorDb  // 0 at floor, 1 at 0 dB
            let c = Colormap.color(t)
            colors += [Float(c.r), Float(c.g), Float(c.b), 0.75]
        }
        let colorData = colors.withUnsafeBufferPointer { Data(buffer: $0) }
        let colorSource = SCNGeometrySource(data: colorData, semantic: .color, vectorCount: verts.count,
                                            usesFloatComponents: true, componentsPerVector: 4,
                                            bytesPerComponent: 4, dataOffset: 0, dataStride: 16)
        let geom = SCNGeometry(sources: [SCNGeometrySource(vertices: verts), colorSource],
                               elements: [SCNGeometryElement(indices: mesh.indices, primitiveType: .triangles)])
        let mat = SCNMaterial()
        mat.lightingModel = .constant
        mat.isDoubleSided = true
        mat.transparency = 0.8
        mat.writesToDepthBuffer = false
        geom.materials = [mat]
        let node = SCNNode(geometry: geom)
        node.renderingOrder = 10
        return node
    }
}

/// Perceptually ordered colormap (viridis-like) for SPL maps and balloons.
enum Colormap {
    private static let stops: [(Double, Double, Double)] = [
        (0.267, 0.005, 0.329), (0.283, 0.141, 0.458), (0.254, 0.265, 0.530), (0.207, 0.372, 0.553),
        (0.164, 0.471, 0.558), (0.128, 0.567, 0.551), (0.135, 0.659, 0.518), (0.267, 0.749, 0.441),
        (0.478, 0.821, 0.318), (0.741, 0.873, 0.150), (0.993, 0.906, 0.144),
    ]

    static func color(_ t: Double) -> (r: Double, g: Double, b: Double) {
        let x = min(max(t, 0), 1) * Double(stops.count - 1)
        let i = min(Int(x), stops.count - 2)
        let f = x - Double(i)
        let a = stops[i], b = stops[i + 1]
        return (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f, a.2 + (b.2 - a.2) * f)
    }
}
