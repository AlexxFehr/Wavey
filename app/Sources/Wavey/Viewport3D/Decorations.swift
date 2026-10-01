import AppKit
import SceneKit
import WaveyCore

/// Builds SceneKit nodes for speakers, balloons, microphones and cut planes.
/// All nodes are created in model coordinates (z up).
@MainActor
enum Decorations {
    static func speakers(_ speakers: [SpeakerRec], meshes: [String: CabinetMeshRec], selected: Set<UInt64>,
                         balloon: (UInt64, BalloonMeshRec)?, into root: SCNNode) {
        for s in speakers {
            let node = SCNNode()
            node.name = "spk:\(s.id)"
            node.simdTransform = transform(s)
            let isSelected = selected.contains(s.id)
            let color: NSColor = s.muted ? .darkGray : isSelected ? .systemOrange : NSColor(calibratedWhite: 0.18, alpha: 1)

            // Cabinet in the local frame: x on-axis, y left, z up.
            var center = SIMD3<Float>(0, 0, 0)
            if let mesh = meshes[s.modelId] {
                node.addChildNode(cabinetNode(mesh, color: color, edgeColor: isSelected ? .white : .systemGray))
                center = SIMD3(Float(mesh.acousticCenter.x), Float(mesh.acousticCenter.y), Float(mesh.acousticCenter.z))
            }
            // Aim arrow from the acoustic centre along +x.
            let aim = arrow(length: 1.2, color: isSelected ? .systemOrange : .systemBlue)
            aim.simdPosition = center
            node.addChildNode(aim)

            if let (id, mesh) = balloon, id == s.id {
                let b = balloonNode(mesh, scale: 1.5)
                b.simdPosition = center
                node.addChildNode(b)
            }
            root.addChildNode(node)
        }
    }

    /// Solid cabinet with flat shading plus its outline edges.
    static func cabinetNode(_ mesh: CabinetMeshRec, color: NSColor, edgeColor: NSColor) -> SCNNode {
        let p = mesh.positions
        let v = { (i: UInt32) in SIMD3<Float>(p[Int(i) * 3], p[Int(i) * 3 + 1], p[Int(i) * 3 + 2]) }
        var verts: [SCNVector3] = []
        var normals: [SCNVector3] = []
        for t in stride(from: 0, to: mesh.indices.count, by: 3) {
            let (a, b, c) = (v(mesh.indices[t]), v(mesh.indices[t + 1]), v(mesh.indices[t + 2]))
            let n = simd_normalize(simd_cross(b - a, c - a))
            for q in [a, b, c] {
                verts.append(SCNVector3(q))
                normals.append(SCNVector3(n))
            }
        }
        let solid = SCNGeometry(sources: [SCNGeometrySource(vertices: verts), SCNGeometrySource(normals: normals)],
                                elements: [SCNGeometryElement(indices: (0..<UInt32(verts.count)).map { $0 }, primitiveType: .triangles)])
        let mat = SCNMaterial()
        mat.diffuse.contents = color
        mat.lightingModel = .lambert
        solid.materials = [mat]
        let node = SCNNode(geometry: solid)

        let e = mesh.edgePositions
        if e.count >= 6 {
            let lineVerts = stride(from: 0, to: e.count, by: 3).map { SCNVector3(e[$0], e[$0 + 1], e[$0 + 2]) }
            let lines = SCNGeometry(sources: [SCNGeometrySource(vertices: lineVerts)],
                                    elements: [SCNGeometryElement(indices: (0..<UInt32(lineVerts.count)).map { $0 }, primitiveType: .line)])
            let lm = SCNMaterial()
            lm.diffuse.contents = edgeColor
            lm.lightingModel = .constant
            lines.materials = [lm]
            node.addChildNode(SCNNode(geometry: lines))
        }
        return node
    }

    static func microphones(_ mics: [MicrophoneRec], selected: Set<UInt64>, into root: SCNNode) {
        for m in mics {
            let sphere = SCNSphere(radius: 0.12)
            sphere.firstMaterial?.diffuse.contents = selected.contains(m.id) ? NSColor.systemOrange : NSColor.systemPink
            sphere.firstMaterial?.lightingModel = .lambert
            let node = SCNNode(geometry: sphere)
            node.name = "mic:\(m.id)"
            node.position = SCNVector3(m.position.x, m.position.y, m.position.z)
            let stand = SCNCylinder(radius: 0.015, height: m.position.z)
            stand.firstMaterial?.diffuse.contents = NSColor.secondaryLabelColor
            let standNode = SCNNode(geometry: stand)
            standNode.eulerAngles = SCNVector3(Double.pi / 2, 0, 0)  // cylinder y → z
            standNode.position = SCNVector3(0, 0, -m.position.z / 2)
            node.addChildNode(standNode)
            root.addChildNode(node)
        }
    }

    /// Rotation taking a plane's local (u, v, normal) axes to model space.
    static func planeTransform(axis: PlaneAxisRec, center: SIMD3<Float>) -> simd_float4x4 {
        let (u, v, n): (SIMD3<Float>, SIMD3<Float>, SIMD3<Float>) = switch axis {
        case .horizontal: ([1, 0, 0], [0, 1, 0], [0, 0, 1])
        case .constX: ([0, 1, 0], [0, 0, 1], [1, 0, 0])
        case .constY: ([1, 0, 0], [0, 0, 1], [0, -1, 0])
        }
        return simd_float4x4(columns: (SIMD4(u, 0), SIMD4(v, 0), SIMD4(n, 0), SIMD4(center, 1)))
    }

    /// Outline of a cut plane before results exist, sized to the room bounds.
    static func planeOutline(_ p: CutPlaneRec, room: RoomSnapshot, into root: SCNNode) {
        let lo = room.boundsMin, hi = room.boundsMax
        let (w, h, cu, cv): (Double, Double, Double, Double) = switch p.axis {
        case .horizontal: (hi.x - lo.x, hi.y - lo.y, (lo.x + hi.x) / 2, (lo.y + hi.y) / 2)
        case .constX: (hi.y - lo.y, hi.z - lo.z, (lo.y + hi.y) / 2, (lo.z + hi.z) / 2)
        case .constY: (hi.x - lo.x, hi.z - lo.z, (lo.x + hi.x) / 2, (lo.z + hi.z) / 2)
        }
        let plane = SCNPlane(width: w, height: h)
        plane.firstMaterial?.diffuse.contents = NSColor.systemTeal.withAlphaComponent(0.25)
        plane.firstMaterial?.isDoubleSided = true
        plane.firstMaterial?.lightingModel = .constant
        plane.firstMaterial?.writesToDepthBuffer = false
        let node = SCNNode(geometry: plane)
        node.simdTransform = planeTransform(axis: p.axis, center: center(p.axis, p.offset, cu, cv))
        node.renderingOrder = 5
        root.addChildNode(node)
    }

    static func center(_ axis: PlaneAxisRec, _ offset: Double, _ cu: Double, _ cv: Double) -> SIMD3<Float> {
        switch axis {
        case .horizontal: SIMD3(Float(cu), Float(cv), Float(offset))
        case .constX: SIMD3(Float(offset), Float(cu), Float(cv))
        case .constY: SIMD3(Float(cu), Float(offset), Float(cv))
        }
    }

    /// SPL heatmap of a cut plane as a textured quad.
    static func heatmap(_ map: PlaneMapRec, range: ClosedRange<Double>, into root: SCNNode) {
        guard let image = heatmapImage(map, range: range) else { return }
        let w = Double(map.nu) * map.du, h = Double(map.nv) * map.dv
        let plane = SCNPlane(width: w, height: h)
        let m = plane.firstMaterial!
        m.diffuse.contents = image
        m.diffuse.magnificationFilter = .linear
        m.isDoubleSided = true
        m.lightingModel = .constant
        let node = SCNNode(geometry: plane)
        let cu = map.u0 + (Double(map.nu) - 1) / 2 * map.du
        let cv = map.v0 + (Double(map.nv) - 1) / 2 * map.dv
        node.simdTransform = planeTransform(axis: map.axis, center: center(map.axis, map.offset, cu, cv))
        node.renderingOrder = 5
        root.addChildNode(node)
    }

    static func heatmapImage(_ map: PlaneMapRec, range: ClosedRange<Double>) -> CGImage? {
        let (nu, nv) = (Int(map.nu), Int(map.nv))
        var px = [UInt8](repeating: 0, count: nu * nv * 4)
        let span = max(range.upperBound - range.lowerBound, 1e-6)
        for iv in 0..<nv {
            let row = nv - 1 - iv  // image rows run top-down, v runs up
            for iu in 0..<nu {
                let v = Double(map.valuesDb[iv * nu + iu])
                let o = (row * nu + iu) * 4
                guard v.isFinite else { continue }
                let c = Colormap.color((v - range.lowerBound) / span)
                px[o] = UInt8(c.r * 255); px[o + 1] = UInt8(c.g * 255); px[o + 2] = UInt8(c.b * 255); px[o + 3] = 235
            }
        }
        let data = Data(px) as CFData
        guard let provider = CGDataProvider(data: data) else { return nil }
        return CGImage(width: nu, height: nv, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: nu * 4,
                       space: CGColorSpace(name: CGColorSpace.sRGB)!,
                       bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
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
