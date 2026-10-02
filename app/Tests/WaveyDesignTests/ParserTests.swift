import SwiftUI
import Testing
@testable import WaveyDesign

@Test func expressions() {
    #expect(ExpressionParser.evaluate("2.4+0.1") == 2.5)
    #expect(ExpressionParser.evaluate("7.2/2") == 3.6)
    #expect(abs(ExpressionParser.evaluate("3*0.6")! - 1.8) < 1e-12)
    #expect(ExpressionParser.evaluate("-(1+2)*2") == -6)
    #expect(ExpressionParser.evaluate("2,5") == 2.5)
    #expect(ExpressionParser.evaluate("1e3") == 1000)
    #expect(ExpressionParser.evaluate("sqrt(16)") == 4)
    #expect(abs(ExpressionParser.evaluate("sin(30)")! - 0.5) < 1e-12)
    #expect(ExpressionParser.evaluate("pi") == .pi)
    #expect(ExpressionParser.evaluate("\u{2212}3") == -3)
    #expect(ExpressionParser.evaluate("2*") == nil)
    #expect(ExpressionParser.evaluate("1/0") == nil)
    #expect(ExpressionParser.evaluate("abc") == nil)
    #expect(ExpressionParser.evaluate("exp(1)") == nil)
}

@Test func units() {
    let len = UnitFormatter(.length)
    #expect(len.parse("250 cm") == 2.5)
    #expect(len.parse("1200mm") == 1.2)
    #expect(abs(len.parse("10 ft")! - 3.048) < 1e-12)
    #expect(len.parse("2.4+0.1 m") == 2.5)
    #expect(len.parse("x") == nil)
    let f = UnitFormatter(.frequency)
    #expect(f.parse("1.5 kHz") == 1500)
    #expect(f.parse("63") == 63)
    let d = UnitFormatter(.delay)
    #expect(d.parse("1.2 ms") == 1.2)
    #expect(d.parse("0.01 s") == 10)
}

@Test func formatting() {
    #expect(UnitFormatter(.length).string(2.5121) == "2.512")
    #expect(UnitFormatter(.angle).string(-23.5) == "\u{2212}23.5")
    #expect(UnitFormatter(.length).string(-0.0001) == "0.000")
    #expect(UnitFormatter(.length).withUnit(1.2) == "1.200 m")
    #expect(UnitFormatter(.level).string(.nan) == "—")
}

/// Text tokens meet 4.5:1 on their surfaces (spec §3.1).
@Test func tokenContrast() {
    func lum(_ rgb: UInt32) -> Double {
        let c = [Double((rgb >> 16) & 0xff), Double((rgb >> 8) & 0xff), Double(rgb & 0xff)].map { v -> Double in
            let s = v / 255
            return s <= 0.03928 ? s / 12.92 : pow((s + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }
    func ratio(_ a: UInt32, _ b: UInt32) -> Double {
        let (x, y) = (lum(a), lum(b))
        return (max(x, y) + 0.05) / (min(x, y) + 0.05)
    }
    let surfaces: [UInt32] = [0x15171A, 0x17191C, 0x1C1E22]
    for text: UInt32 in [0xF1F4F7, 0xD4D9DF, 0x8A949E, 0x6DB3FF] {
        for s in surfaces { #expect(ratio(text, s) >= 4.5, "text \(String(text, radix: 16)) on \(String(s, radix: 16))") }
    }
    #expect(ratio(0xA8D2FF, 0x233B59) >= 4.5)
    #expect(ratio(0xD4D9DF, 0x272A30) >= 4.5)
}
