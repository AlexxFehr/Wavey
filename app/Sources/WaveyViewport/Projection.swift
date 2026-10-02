import CoreGraphics
import simd

/// View and projection for one frame; also used by the label overlay and
/// for turning pointer positions into rays.
public struct Projection: Sendable {
    public let view: simd_float4x4
    public let proj: simd_float4x4
    public let viewProj: simd_float4x4
    public let inverse: simd_float4x4
    public let size: CGSize
    public let eye: SIMD3<Double>
    public let ortho: Bool
    /// World units per point at the target (ortho) — for pick tolerances.
    public let metresPerPoint: Double

    public init(mode: ViewMode, orbit: OrbitCamera, plan: PlanCamera, gridZ: Double, size: CGSize) {
        let w = max(Double(size.width), 1), h = max(Double(size.height), 1)
        let aspect = w / h
        switch mode {
        case .plan:
            let halfW = w / 2 / plan.pointsPerMetre, halfH = h / 2 / plan.pointsPerMetre
            let c = SIMD3(plan.center.x, plan.center.y, gridZ)
            let e = c + SIMD3(0, 0, 500)
            view = lookAt(eye: e, target: c, up: SIMD3(0, 1, 0))
            proj = orthographic(left: -halfW, right: halfW, bottom: -halfH, top: halfH, near: 1, far: 1000)
            eye = e
            ortho = true
            metresPerPoint = 1 / plan.pointsPerMetre
        default:
            var o = orbit
            switch mode {
            case .top: o.yaw = 0; o.pitch = 89.9
            case .front: o.yaw = 0; o.pitch = 0
            case .side: o.yaw = 90; o.pitch = 0
            default: break
            }
            let e = o.eye
            let up: SIMD3<Double> = abs(o.pitch) > 89.5 ? SIMD3(0, 1, 0) : SIMD3(0, 0, 1)
            view = lookAt(eye: e, target: o.target, up: up)
            let halfH = o.distance * tan(o.fovDeg * .pi / 360)
            let isOrtho = mode != .perspective || o.orthographic
            let near = max(o.distance * 0.005, 0.01), far = o.distance * 20 + 200
            proj = isOrtho
                ? orthographic(left: -halfH * aspect, right: halfH * aspect, bottom: -halfH, top: halfH, near: -far, far: far)
                : perspective(fovY: o.fovDeg * .pi / 180, aspect: aspect, near: near, far: far)
            eye = e
            ortho = isOrtho
            metresPerPoint = 2 * halfH / h
        }
        viewProj = proj * view
        inverse = viewProj.inverse
        self.size = size
    }

    /// Screen point (top-left origin, points) of a world position; nil if behind the eye.
    public func project(_ p: SIMD3<Float>) -> CGPoint? {
        let c = viewProj * SIMD4(p, 1)
        guard c.w > 1e-6 else { return nil }
        let n = SIMD3(c.x, c.y, c.z) / c.w
        return CGPoint(x: (Double(n.x) + 1) / 2 * size.width, y: (1 - Double(n.y)) / 2 * size.height)
    }

    public func project(_ p: SIMD3<Double>) -> CGPoint? { project(SIMD3<Float>(p)) }

    /// World ray through a screen point.
    public func ray(_ pt: CGPoint) -> (origin: SIMD3<Double>, dir: SIMD3<Double>) {
        let x = Float(pt.x / size.width * 2 - 1), y = Float(1 - pt.y / size.height * 2)
        let a = inverse * SIMD4(x, y, -1, 1), b = inverse * SIMD4(x, y, 1, 1)
        let pa = SIMD3<Double>(SIMD3(a.x, a.y, a.z) / a.w), pb = SIMD3<Double>(SIMD3(b.x, b.y, b.z) / b.w)
        return (pa, simd_normalize(pb - pa))
    }

    /// Intersection of the pointer ray with the horizontal plane z = `z`.
    public func onPlane(_ pt: CGPoint, z: Double) -> SIMD3<Double>? {
        let (o, d) = ray(pt)
        guard abs(d.z) > 1e-9 else { return nil }
        let t = (z - o.z) / d.z
        guard t > -1e6 else { return nil }
        return o + d * t
    }
}

func lookAt(eye: SIMD3<Double>, target: SIMD3<Double>, up: SIMD3<Double>) -> simd_float4x4 {
    let f = simd_normalize(target - eye)
    let s = simd_normalize(simd_cross(f, up))
    let u = simd_cross(s, f)
    let m = simd_double4x4(rows: [
        SIMD4(s.x, s.y, s.z, -simd_dot(s, eye)),
        SIMD4(u.x, u.y, u.z, -simd_dot(u, eye)),
        SIMD4(-f.x, -f.y, -f.z, simd_dot(f, eye)),
        SIMD4(0, 0, 0, 1),
    ])
    return toFloat(m)
}

/// Metal clip space (z in [0, 1]).
func perspective(fovY: Double, aspect: Double, near: Double, far: Double) -> simd_float4x4 {
    let y = 1 / tan(fovY / 2), x = y / aspect, z = far / (near - far)
    return toFloat(simd_double4x4(rows: [
        SIMD4(x, 0, 0, 0),
        SIMD4(0, y, 0, 0),
        SIMD4(0, 0, z, z * near),
        SIMD4(0, 0, -1, 0),
    ]))
}

func orthographic(left: Double, right: Double, bottom: Double, top: Double, near: Double, far: Double) -> simd_float4x4 {
    toFloat(simd_double4x4(rows: [
        SIMD4(2 / (right - left), 0, 0, -(right + left) / (right - left)),
        SIMD4(0, 2 / (top - bottom), 0, -(top + bottom) / (top - bottom)),
        SIMD4(0, 0, -1 / (far - near), -near / (far - near)),
        SIMD4(0, 0, 0, 1),
    ]))
}

func toFloat(_ m: simd_double4x4) -> simd_float4x4 {
    simd_float4x4(columns: (SIMD4<Float>(m.columns.0), SIMD4<Float>(m.columns.1), SIMD4<Float>(m.columns.2), SIMD4<Float>(m.columns.3)))
}
