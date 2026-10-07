import Metal
import MetalKit
import simd

struct FrameUniforms {
    var viewProj: simd_float4x4
    var inverse: simd_float4x4
    var viewport: SIMD2<Float>
    var scale: Float
    var gridZ: Float
    var eye: SIMD3<Float>
    var isPlan: Float
    var gridStep: Float
    var metresPerPoint: Float
    var showGrid: Float
    var ortho: Float
    var bgCentre: SIMD4<Float>
    var bgEdge: SIMD4<Float>
    var gridMinor: SIMD4<Float>
    var gridMajor: SIMD4<Float>
    var gridAxis: SIMD4<Float>
}

struct DrawUniforms {
    var color: SIMD4<Float>
    var id: UInt32
    var lit: Float
    var useVertexColor: Float
    var pad: Float = 0
}

struct LineUniforms {
    var color: SIMD4<Float>
    var width: Float
    var dashed: Float
    var id: UInt32
    var pad: Float = 0
}

struct FieldUniforms {
    var origin: SIMD3<Float>
    var pad0: Float = 0
    var uAxis: SIMD3<Float>
    var pad1: Float = 0
    var vAxis: SIMD3<Float>
    var pad2: Float = 0
    var lo: Float
    var hi: Float
    var opacity: Float
    var contour: Float
    var cells: Float
    var id: UInt32
    /// 1: draped (heights in the vertex texture), 0: flat.
    var draped: Float = 0
    var pad4: Float = 0
}

/// Viewport colours (sRGB, set by the host from design tokens).
public struct ViewportPalette: Sendable {
    public var planBackground: SIMD4<Float>
    public var centre: SIMD4<Float>
    public var edge: SIMD4<Float>
    public var gridMinor: SIMD4<Float>
    public var gridMajor: SIMD4<Float>
    public var gridMinor3D: SIMD4<Float>
    public var gridAxis: SIMD4<Float>
    public init(planBackground: SIMD4<Float>, centre: SIMD4<Float>, edge: SIMD4<Float>, gridMinor: SIMD4<Float>,
                gridMajor: SIMD4<Float>, gridMinor3D: SIMD4<Float>, gridAxis: SIMD4<Float>) {
        self.planBackground = planBackground
        self.centre = centre
        self.edge = edge
        self.gridMinor = gridMinor
        self.gridMajor = gridMajor
        self.gridMinor3D = gridMinor3D
        self.gridAxis = gridAxis
    }
}

/// Draws a `SceneSnapshot`. GPU buffers are rebuilt only when the snapshot
/// revision changes; field textures only when their version changes.
@MainActor
final class Renderer {
    let device: MTLDevice
    let queue: MTLCommandQueue
    let sampleCount = 4
    private var bg: MTLRenderPipelineState!
    private var grid: MTLRenderPipelineState!
    private var meshOpaque: MTLRenderPipelineState!
    private var line: MTLRenderPipelineState!
    private var marker: MTLRenderPipelineState!
    private var field: MTLRenderPipelineState!
    private var meshID: MTLRenderPipelineState!
    private var markerID: MTLRenderPipelineState!
    private var fieldID: MTLRenderPipelineState!
    private var depthWrite: MTLDepthStencilState!
    private var depthRead: MTLDepthStencilState!
    private var depthOff: MTLDepthStencilState!
    private var lutSampler: MTLSamplerState!
    private var luts: [ColorMap: MTLTexture] = [:]

    // Per-snapshot GPU data
    private var revision = -1
    private struct MeshBuf { let buffer: MTLBuffer; let count: Int; var draw: DrawUniforms; let depthWrite: Bool; let translucent: Bool }
    private struct LineBuf { let buffer: MTLBuffer; let count: Int; var draw: LineUniforms; let overlay: Bool }
    private var opaqueMeshes: [MeshBuf] = []
    private var translucentMeshes: [MeshBuf] = []
    private var lineBufs: [LineBuf] = []
    private var markerBuf: MTLBuffer?
    private var markerCount = 0
    private struct FieldTex { var version: Int; var texture: MTLTexture; var heights: MTLTexture? }
    private var fieldTextures: [UInt32: FieldTex] = [:]
    private var fields: [FieldQuad] = []
    private var pickTargets: (MTLTexture, MTLTexture)?

    var palette: ViewportPalette

    init?(palette: ViewportPalette) {
        guard let d = MTLCreateSystemDefaultDevice(), let q = d.makeCommandQueue() else { return nil }
        device = d
        queue = q
        self.palette = palette
        do { try buildPipelines() } catch {
            NSLog("Wavey viewport: shader build failed: \(error)")
            return nil
        }
    }

    private func buildPipelines() throws {
        let lib = try device.makeLibrary(source: shaderSource, options: nil)
        func pipe(_ vs: String, _ fs: String, blend: Bool, color: MTLPixelFormat = .bgra8Unorm, samples: Int? = nil,
                  depth: Bool = true) throws -> MTLRenderPipelineState {
            let d = MTLRenderPipelineDescriptor()
            d.vertexFunction = lib.makeFunction(name: vs)
            d.fragmentFunction = lib.makeFunction(name: fs)
            d.colorAttachments[0].pixelFormat = color
            d.rasterSampleCount = samples ?? sampleCount
            if depth { d.depthAttachmentPixelFormat = .depth32Float }
            if blend {
                let c = d.colorAttachments[0]!
                c.isBlendingEnabled = true
                c.sourceRGBBlendFactor = .sourceAlpha
                c.destinationRGBBlendFactor = .oneMinusSourceAlpha
                c.sourceAlphaBlendFactor = .one
                c.destinationAlphaBlendFactor = .oneMinusSourceAlpha
            }
            return try device.makeRenderPipelineState(descriptor: d)
        }
        bg = try pipe("bg_vs", "bg_fs", blend: false)
        grid = try pipe("grid_vs", "grid_fs", blend: true)
        meshOpaque = try pipe("mesh_vs", "mesh_fs", blend: true)
        line = try pipe("line_vs", "line_fs", blend: true)
        marker = try pipe("marker_vs", "marker_fs", blend: true)
        field = try pipe("field_vs", "field_fs", blend: true)
        meshID = try pipe("mesh_vs", "mesh_id_fs", blend: false, color: .r32Uint, samples: 1)
        markerID = try pipe("marker_vs", "marker_id_fs", blend: false, color: .r32Uint, samples: 1)
        fieldID = try pipe("field_vs", "field_id_fs", blend: false, color: .r32Uint, samples: 1)

        func depthState(_ write: Bool, _ test: Bool) -> MTLDepthStencilState {
            let d = MTLDepthStencilDescriptor()
            d.depthCompareFunction = test ? .lessEqual : .always
            d.isDepthWriteEnabled = write
            return device.makeDepthStencilState(descriptor: d)!
        }
        depthWrite = depthState(true, true)
        depthRead = depthState(false, true)
        depthOff = depthState(false, false)
        let s = MTLSamplerDescriptor()
        s.minFilter = .linear
        s.magFilter = .linear
        s.sAddressMode = .clampToEdge
        lutSampler = device.makeSamplerState(descriptor: s)
    }

    private func lut(_ m: ColorMap) -> MTLTexture? {
        if let t = luts[m] { return t }
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm, width: 256, height: 1, mipmapped: false)
        guard let t = device.makeTexture(descriptor: d) else { return nil }
        let bytes = m.lut
        bytes.withUnsafeBytes { t.replace(region: MTLRegionMake2D(0, 0, 256, 1), mipmapLevel: 0, withBytes: $0.baseAddress!, bytesPerRow: 256 * 4) }
        luts[m] = t
        return t
    }

    // MARK: Upload

    func upload(_ scene: SceneSnapshot) {
        if scene.revision == revision { return }
        revision = scene.revision
        opaqueMeshes = []
        translucentMeshes = []
        func meshBuffer(_ positions: [Float], normals: (Int) -> SIMD3<Float>, colors: [Float], color: SIMD4<Float>) -> MTLBuffer? {
            let n = positions.count / 3
            guard n > 0 else { return nil }
            var v = [Float](repeating: 0, count: n * 10)
            for i in 0..<n {
                v[i * 10] = positions[i * 3]; v[i * 10 + 1] = positions[i * 3 + 1]; v[i * 10 + 2] = positions[i * 3 + 2]
                let nm = normals(i)
                v[i * 10 + 3] = nm.x; v[i * 10 + 4] = nm.y; v[i * 10 + 5] = nm.z
                if colors.count >= (i + 1) * 4 {
                    for k in 0..<4 { v[i * 10 + 6 + k] = colors[i * 4 + k] }
                } else {
                    v[i * 10 + 6] = color.x; v[i * 10 + 7] = color.y; v[i * 10 + 8] = color.z; v[i * 10 + 9] = color.w
                }
            }
            return device.makeBuffer(bytes: v, length: v.count * 4)
        }
        for s in scene.surfaces {
            guard let b = meshBuffer(s.positions, normals: { _ in s.normal }, colors: [], color: s.color) else { continue }
            let lit: Float = s.style == .ghost ? 0 : 1
            let m = MeshBuf(buffer: b, count: s.positions.count / 3, draw: DrawUniforms(color: s.color, id: s.id, lit: lit, useVertexColor: 0),
                            depthWrite: s.color.w > 0.95, translucent: s.color.w <= 0.95)
            if m.translucent { translucentMeshes.append(m) } else { opaqueMeshes.append(m) }
        }
        for mesh in scene.meshes {
            let p = mesh.positions
            let normal: (Int) -> SIMD3<Float> = { i in
                let t = i / 3 * 9
                guard t + 8 < p.count else { return SIMD3(0, 0, 1) }
                let a = SIMD3(p[t], p[t + 1], p[t + 2]), b = SIMD3(p[t + 3], p[t + 4], p[t + 5]), c = SIMD3(p[t + 6], p[t + 7], p[t + 8])
                let n = simd_cross(b - a, c - a)
                return simd_length(n) > 1e-12 ? simd_normalize(n) : SIMD3(0, 0, 1)
            }
            guard let b = meshBuffer(p, normals: normal, colors: mesh.colors, color: mesh.color) else { continue }
            let m = MeshBuf(buffer: b, count: p.count / 3,
                            draw: DrawUniforms(color: mesh.color, id: mesh.id, lit: mesh.lit ? 1 : 0, useVertexColor: mesh.colors.isEmpty ? 0 : 1),
                            depthWrite: mesh.depthWrite, translucent: !mesh.depthWrite || mesh.color.w < 0.95)
            if m.translucent { translucentMeshes.append(m) } else { opaqueMeshes.append(m) }
        }
        lineBufs = scene.lines.compactMap { l in
            guard l.segments.count >= 6, let b = device.makeBuffer(bytes: l.segments, length: l.segments.count * 4) else { return nil }
            return LineBuf(buffer: b, count: l.segments.count / 6,
                           draw: LineUniforms(color: l.color, width: l.width, dashed: l.dashed ? 1 : 0, id: l.id), overlay: l.overlay)
        }
        var mk = [Float]()
        mk.reserveCapacity(scene.markers.count * 16)
        for m in scene.markers {
            mk += [m.position.x, m.position.y, m.position.z, m.size, m.color.x, m.color.y, m.color.z, m.color.w,
                   m.fill.x, m.fill.y, m.fill.z, m.fill.w,
                   Float(bitPattern: UInt32(m.shape.rawValue)), Float(bitPattern: m.id), 0, 0]
        }
        markerCount = scene.markers.count
        markerBuf = mk.isEmpty ? nil : device.makeBuffer(bytes: mk, length: mk.count * 4)

        fields = scene.fields
        var keep: [UInt32: FieldTex] = [:]
        for f in scene.fields {
            if let t = fieldTextures[f.id], t.version == f.version, t.texture.width == f.nu, t.texture.height == f.nv {
                keep[f.id] = t
                continue
            }
            let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .r32Float, width: max(f.nu, 1), height: max(f.nv, 1), mipmapped: false)
            d.usage = .shaderRead
            guard let tex = device.makeTexture(descriptor: d), f.values.count >= f.nu * f.nv else { continue }
            f.values.withUnsafeBytes {
                tex.replace(region: MTLRegionMake2D(0, 0, f.nu, f.nv), mipmapLevel: 0, withBytes: $0.baseAddress!, bytesPerRow: f.nu * 4)
            }
            var heights: MTLTexture?
            if f.heights.count >= f.nu * f.nv, let h = device.makeTexture(descriptor: d) {
                f.heights.withUnsafeBytes {
                    h.replace(region: MTLRegionMake2D(0, 0, f.nu, f.nv), mipmapLevel: 0, withBytes: $0.baseAddress!, bytesPerRow: f.nu * 4)
                }
                heights = h
            }
            keep[f.id] = FieldTex(version: f.version, texture: tex, heights: heights)
        }
        fieldTextures = keep
    }

    // MARK: Draw

    func frameUniforms(_ p: Projection, mode: ViewMode, scene: SceneSnapshot, scale: Float) -> FrameUniforms {
        let plan = mode == .plan
        let pal = palette
        let step = gridStep(base: scene.gridStep, metresPerPoint: p.metresPerPoint)
        return FrameUniforms(
            viewProj: p.viewProj, inverse: p.inverse,
            viewport: SIMD2(Float(p.size.width), Float(p.size.height)), scale: scale,
            gridZ: Float(scene.gridZ), eye: SIMD3<Float>(p.eye), isPlan: plan ? 1 : 0, gridStep: Float(step),
            metresPerPoint: Float(p.metresPerPoint), showGrid: scene.showGrid ? 1 : 0, ortho: p.ortho ? 1 : 0,
            bgCentre: plan ? pal.planBackground : pal.centre, bgEdge: pal.edge,
            gridMinor: plan ? pal.gridMinor : pal.gridMinor3D, gridMajor: pal.gridMajor, gridAxis: pal.gridAxis)
    }

    /// Encodes the colour pass.
    func encode(_ enc: MTLRenderCommandEncoder, frame: inout FrameUniforms) {
        let fl = MemoryLayout<FrameUniforms>.stride
        enc.setDepthStencilState(depthOff)
        enc.setRenderPipelineState(bg)
        enc.setFragmentBytes(&frame, length: fl, index: 0)
        enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)

        enc.setDepthStencilState(depthRead)
        enc.setRenderPipelineState(grid)
        enc.setFragmentBytes(&frame, length: fl, index: 0)
        enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)

        enc.setCullMode(.none)
        enc.setRenderPipelineState(meshOpaque)
        for var m in opaqueMeshes {
            enc.setDepthStencilState(m.depthWrite ? depthWrite : depthRead)
            enc.setVertexBuffer(m.buffer, offset: 0, index: 0)
            enc.setVertexBytes(&frame, length: fl, index: 1)
            enc.setVertexBytes(&m.draw, length: MemoryLayout<DrawUniforms>.stride, index: 2)
            enc.setFragmentBytes(&frame, length: fl, index: 0)
            enc.setFragmentBytes(&m.draw, length: MemoryLayout<DrawUniforms>.stride, index: 1)
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: m.count)
        }

        // Fields (translucent, before translucent walls so walls ghost over them).
        enc.setDepthStencilState(depthRead)
        enc.setRenderPipelineState(field)
        for f in fields {
            guard let ft = fieldTextures[f.id], let lut = lut(f.colorMap) else { continue }
            var u = FieldUniforms(origin: f.origin, uAxis: f.uAxis, vAxis: f.vAxis, lo: f.range.lowerBound, hi: f.range.upperBound,
                                  opacity: f.opacity, contour: f.contourStep, cells: f.cells ? 1 : 0, id: f.id, draped: ft.heights == nil ? 0 : 1)
            enc.setVertexBytes(&frame, length: fl, index: 1)
            enc.setVertexBytes(&u, length: MemoryLayout<FieldUniforms>.stride, index: 2)
            enc.setVertexTexture(ft.heights ?? ft.texture, index: 0)
            enc.setFragmentBytes(&u, length: MemoryLayout<FieldUniforms>.stride, index: 1)
            enc.setFragmentTexture(ft.texture, index: 0)
            enc.setFragmentTexture(lut, index: 1)
            enc.setFragmentSamplerState(lutSampler, index: 0)
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: ft.heights == nil ? 6 : 6 * f.nu * f.nv)
        }

        enc.setRenderPipelineState(meshOpaque)
        enc.setDepthStencilState(depthRead)
        for var m in translucentMeshes {
            enc.setVertexBuffer(m.buffer, offset: 0, index: 0)
            enc.setVertexBytes(&frame, length: fl, index: 1)
            enc.setVertexBytes(&m.draw, length: MemoryLayout<DrawUniforms>.stride, index: 2)
            enc.setFragmentBytes(&frame, length: fl, index: 0)
            enc.setFragmentBytes(&m.draw, length: MemoryLayout<DrawUniforms>.stride, index: 1)
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: m.count)
        }

        enc.setRenderPipelineState(line)
        for overlay in [false, true] {
            enc.setDepthStencilState(overlay ? depthOff : depthRead)
            for var l in lineBufs where l.overlay == overlay {
                enc.setVertexBuffer(l.buffer, offset: 0, index: 0)
                enc.setVertexBytes(&frame, length: fl, index: 1)
                enc.setVertexBytes(&l.draw, length: MemoryLayout<LineUniforms>.stride, index: 2)
                enc.setFragmentBytes(&l.draw, length: MemoryLayout<LineUniforms>.stride, index: 1)
                enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 6, instanceCount: l.count)
            }
        }

        if let mb = markerBuf, markerCount > 0 {
            enc.setDepthStencilState(depthOff)
            enc.setRenderPipelineState(marker)
            enc.setVertexBuffer(mb, offset: 0, index: 0)
            enc.setVertexBytes(&frame, length: fl, index: 1)
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 6, instanceCount: markerCount)
        }
    }

    /// Renders entity ids into an r32Uint target and returns the id at `pt`
    /// (points, top-left origin), or nil. Synchronous: one small offscreen pass.
    func pick(at pt: CGPoint, projection p: Projection, mode: ViewMode, scene: SceneSnapshot, scale: Float, radius: Int = 0) -> [UInt32] {
        let w = max(Int(p.size.width), 1), h = max(Int(p.size.height), 1)
        if pickTargets?.0.width != w || pickTargets?.0.height != h {
            let cd = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .r32Uint, width: w, height: h, mipmapped: false)
            cd.usage = .renderTarget
            cd.storageMode = .private
            let dd = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .depth32Float, width: w, height: h, mipmapped: false)
            dd.usage = .renderTarget
            dd.storageMode = .private
            if let c = device.makeTexture(descriptor: cd), let d = device.makeTexture(descriptor: dd) { pickTargets = (c, d) }
        }
        guard let (color, depth) = pickTargets, let cb = queue.makeCommandBuffer() else { return [] }
        let rp = MTLRenderPassDescriptor()
        rp.colorAttachments[0].texture = color
        rp.colorAttachments[0].loadAction = .clear
        rp.colorAttachments[0].clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 0)
        rp.colorAttachments[0].storeAction = .store
        rp.depthAttachment.texture = depth
        rp.depthAttachment.clearDepth = 1
        rp.depthAttachment.storeAction = .dontCare
        guard let enc = cb.makeRenderCommandEncoder(descriptor: rp) else { return [] }
        var frame = frameUniforms(p, mode: mode, scene: scene, scale: 1)
        let fl = MemoryLayout<FrameUniforms>.stride
        enc.setCullMode(.none)
        enc.setDepthStencilState(depthWrite)
        enc.setRenderPipelineState(meshID)
        for var m in opaqueMeshes + translucentMeshes where m.draw.id != 0 {
            enc.setVertexBuffer(m.buffer, offset: 0, index: 0)
            enc.setVertexBytes(&frame, length: fl, index: 1)
            enc.setVertexBytes(&m.draw, length: MemoryLayout<DrawUniforms>.stride, index: 2)
            enc.setFragmentBytes(&m.draw, length: MemoryLayout<DrawUniforms>.stride, index: 1)
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: m.count)
        }
        enc.setRenderPipelineState(fieldID)
        for f in fields where f.id != 0 {
            guard let ft = fieldTextures[f.id] else { continue }
            var u = FieldUniforms(origin: f.origin, uAxis: f.uAxis, vAxis: f.vAxis, lo: 0, hi: 1, opacity: 1, contour: 0, cells: 1, id: f.id,
                                  draped: ft.heights == nil ? 0 : 1)
            enc.setVertexBytes(&frame, length: fl, index: 1)
            enc.setVertexBytes(&u, length: MemoryLayout<FieldUniforms>.stride, index: 2)
            enc.setVertexTexture(ft.heights ?? ft.texture, index: 0)
            enc.setFragmentBytes(&u, length: MemoryLayout<FieldUniforms>.stride, index: 1)
            enc.setFragmentTexture(ft.texture, index: 0)
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: ft.heights == nil ? 6 : 6 * f.nu * f.nv)
        }
        if let mb = markerBuf, markerCount > 0 {
            enc.setDepthStencilState(depthOff)
            enc.setRenderPipelineState(markerID)
            enc.setVertexBuffer(mb, offset: 0, index: 0)
            enc.setVertexBytes(&frame, length: fl, index: 1)
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 6, instanceCount: markerCount)
        }
        enc.endEncoding()
        let x0 = max(0, min(w - 1, Int(pt.x) - radius)), y0 = max(0, min(h - 1, Int(pt.y) - radius))
        let rw = min(w - x0, radius * 2 + 1), rh = min(h - y0, radius * 2 + 1)
        guard let out = device.makeBuffer(length: rw * rh * 4, options: .storageModeShared), let blit = cb.makeBlitCommandEncoder() else { return [] }
        blit.copy(from: color, sourceSlice: 0, sourceLevel: 0, sourceOrigin: MTLOrigin(x: x0, y: y0, z: 0),
                  sourceSize: MTLSize(width: rw, height: rh, depth: 1), to: out, destinationOffset: 0,
                  destinationBytesPerRow: rw * 4, destinationBytesPerImage: rw * rh * 4)
        blit.endEncoding()
        cb.commit()
        cb.waitUntilCompleted()
        let ptr = out.contents().bindMemory(to: UInt32.self, capacity: rw * rh)
        var ids: [UInt32] = []
        // Centre first, then outward.
        let cx = Int(pt.x) - x0, cy = Int(pt.y) - y0
        var order = [(Int, Int)]()
        for yy in 0..<rh { for xx in 0..<rw { order.append((xx, yy)) } }
        order.sort { abs($0.0 - cx) + abs($0.1 - cy) < abs($1.0 - cx) + abs($1.1 - cy) }
        for (xx, yy) in order {
            let v = ptr[yy * rw + xx]
            if v != 0 && !ids.contains(v) { ids.append(v) }
        }
        return ids
    }

    /// Renders the scene offscreen (for snapshots).
    func image(projection p: Projection, mode: ViewMode, scene: SceneSnapshot, scale: CGFloat) -> CGImage? {
        upload(scene)
        let w = Int(p.size.width * scale), h = Int(p.size.height * scale)
        guard w > 0, h > 0 else { return nil }
        let msd = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm, width: w, height: h, mipmapped: false)
        msd.textureType = .type2DMultisample
        msd.sampleCount = sampleCount
        msd.usage = .renderTarget
        msd.storageMode = .private
        let rd = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm, width: w, height: h, mipmapped: false)
        rd.usage = [.renderTarget, .shaderRead]
        rd.storageMode = .managed
        let dd = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .depth32Float, width: w, height: h, mipmapped: false)
        dd.textureType = .type2DMultisample
        dd.sampleCount = sampleCount
        dd.usage = .renderTarget
        dd.storageMode = .private
        guard let ms = device.makeTexture(descriptor: msd), let resolve = device.makeTexture(descriptor: rd),
              let depth = device.makeTexture(descriptor: dd), let cb = queue.makeCommandBuffer() else { return nil }
        let rp = MTLRenderPassDescriptor()
        rp.colorAttachments[0].texture = ms
        rp.colorAttachments[0].resolveTexture = resolve
        rp.colorAttachments[0].loadAction = .clear
        rp.colorAttachments[0].storeAction = .multisampleResolve
        rp.depthAttachment.texture = depth
        rp.depthAttachment.clearDepth = 1
        guard let enc = cb.makeRenderCommandEncoder(descriptor: rp) else { return nil }
        var frame = frameUniforms(p, mode: mode, scene: scene, scale: Float(scale))
        encode(enc, frame: &frame)
        enc.endEncoding()
        if let blit = cb.makeBlitCommandEncoder() {
            blit.synchronize(resource: resolve)
            blit.endEncoding()
        }
        cb.commit()
        cb.waitUntilCompleted()
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        resolve.getBytes(&bytes, bytesPerRow: w * 4, from: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0)
        guard let ctx = CGContext(data: &bytes, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        else { return nil }
        return ctx.makeImage()
    }
}

/// Grid spacing: the requested step (or 1-2-5 auto), coarsened so lines are
/// never closer than 12 pt.
func gridStep(base: Double, metresPerPoint mpp: Double) -> Double {
    let minimum = mpp * 12
    if base > 0 {
        var s = base
        var k = 0
        while s < minimum && s < 1e5 { s *= k % 2 == 0 ? 2 : 5; k += 1 }
        return s
    }
    let e = floor(log10(max(minimum, 1e-6)))
    for m in [1.0, 2.0, 5.0, 10.0] where m * pow(10, e) >= minimum { return m * pow(10, e) }
    return 10 * pow(10, e)
}
