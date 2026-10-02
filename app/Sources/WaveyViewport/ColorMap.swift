import CoreGraphics
import Foundation

/// Perceptual colour maps for field display (256-entry LUTs).
public enum ColorMap: String, Codable, Sendable, CaseIterable {
    case inferno, viridis, turbo, magma, grayscale, twilight

    public var title: String {
        switch self {
        case .inferno: "Inferno"
        case .viridis: "Viridis"
        case .turbo: "Turbo"
        case .magma: "Magma"
        case .grayscale: "Grayscale"
        case .twilight: "Twilight (diverging)"
        }
    }

    /// Control points (0–255 RGB), evenly spaced.
    var stops: [(Double, Double, Double)] {
        switch self {
        case .inferno:
            [(0, 0, 4), (40, 11, 84), (101, 21, 110), (159, 42, 99), (212, 72, 66), (245, 125, 21), (250, 193, 39), (252, 255, 164)]
        case .viridis:
            [(68, 1, 84), (72, 40, 120), (62, 74, 137), (49, 104, 142), (38, 130, 142), (31, 158, 137), (53, 183, 121), (109, 205, 89), (180, 222, 44), (253, 231, 37)]
        case .magma:
            [(0, 0, 4), (28, 16, 68), (79, 18, 123), (129, 37, 129), (181, 54, 122), (229, 80, 100), (251, 135, 97), (254, 194, 135), (252, 253, 191)]
        case .turbo:
            [(48, 18, 59), (70, 107, 227), (40, 187, 236), (49, 242, 153), (162, 252, 60), (237, 208, 58), (251, 128, 34), (208, 47, 5), (122, 4, 3)]
        case .grayscale:
            [(16, 16, 16), (240, 240, 240)]
        case .twilight:
            [(47, 20, 54), (90, 52, 140), (110, 120, 200), (170, 190, 220), (226, 217, 226), (220, 180, 160), (190, 110, 80), (150, 45, 60), (47, 20, 54)]
        }
    }

    /// 256 RGBA8 entries.
    public var lut: [UInt8] {
        let s = stops
        var out = [UInt8](repeating: 255, count: 256 * 4)
        for i in 0..<256 {
            let t = Double(i) / 255 * Double(s.count - 1)
            let k = min(Int(t), s.count - 2)
            let f = t - Double(k)
            let a = s[k], b = s[k + 1]
            out[i * 4] = UInt8((a.0 + (b.0 - a.0) * f).rounded())
            out[i * 4 + 1] = UInt8((a.1 + (b.1 - a.1) * f).rounded())
            out[i * 4 + 2] = UInt8((a.2 + (b.2 - a.2) * f).rounded())
        }
        return out
    }

    /// Colour at t in [0, 1].
    public func color(_ t: Double) -> (r: Double, g: Double, b: Double) {
        let l = lut
        let i = min(max(Int((t * 255).rounded()), 0), 255)
        return (Double(l[i * 4]) / 255, Double(l[i * 4 + 1]) / 255, Double(l[i * 4 + 2]) / 255)
    }

    /// Horizontal gradient image for previews and the colour bar.
    public func image(width: Int = 256, vertical: Bool = false) -> CGImage? {
        let l = lut
        let n = vertical ? 256 : width
        var px = [UInt8](repeating: 255, count: n * 4)
        for i in 0..<n {
            let j = vertical ? 255 - i : i * 255 / max(n - 1, 1)
            px[i * 4] = l[j * 4]; px[i * 4 + 1] = l[j * 4 + 1]; px[i * 4 + 2] = l[j * 4 + 2]
        }
        let w = vertical ? 1 : n, h = vertical ? n : 1
        guard let ctx = CGContext(data: &px, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
        else { return nil }
        return ctx.makeImage()
    }
}
