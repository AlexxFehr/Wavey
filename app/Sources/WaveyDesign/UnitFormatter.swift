import Foundation

/// Physical quantity of a numeric field. Values are SI internally (m, Hz,
/// dB, degrees, s); display units are converted only here.
public enum Quantity: Sendable, Hashable {
    case length, frequency, level, angle, time, delay, ratio, percent, count, temperature, plain
}

/// Formats and parses values for display. The only place that converts units.
public struct UnitFormatter: Sendable {
    public var decimals: Int
    public var quantity: Quantity

    public init(_ quantity: Quantity, decimals: Int? = nil) {
        self.quantity = quantity
        self.decimals = decimals ?? UnitFormatter.defaultDecimals(quantity)
    }

    public static func defaultDecimals(_ q: Quantity) -> Int {
        switch q {
        case .length: 3
        case .frequency: 1
        case .level: 1
        case .angle: 1
        case .time: 2
        case .delay: 2
        case .ratio: 2
        case .percent: 0
        case .count: 0
        case .temperature: 1
        case .plain: 2
        }
    }

    /// Unit suffix shown in fields.
    public var unit: String {
        switch quantity {
        case .length: "m"
        case .frequency: "Hz"
        case .level: "dB"
        case .angle: "°"
        case .time: "s"
        case .delay: "ms"
        case .percent: "%"
        case .temperature: "°C"
        case .ratio, .count, .plain: ""
        }
    }

    /// Fixed decimals, thin minus for negatives, no unit.
    public func string(_ v: Double) -> String {
        guard v.isFinite else { return "—" }
        let s = String(format: "%.\(decimals)f", v)
        return UnitFormatter.thinMinus(s == "-" + String(format: "%.\(decimals)f", 0.0) ? String(s.dropFirst()) : s)
    }

    /// Value with its unit ("2.512 m").
    public func withUnit(_ v: Double) -> String {
        unit.isEmpty ? string(v) : "\(string(v)) \(unit)"
    }

    public static func thinMinus(_ s: String) -> String {
        s.replacingOccurrences(of: "-", with: "\u{2212}")
    }

    /// Parses user input: arithmetic, `.` or `,` decimals, thin minus and
    /// unit suffixes for this quantity. Nil if it cannot be parsed.
    public func parse(_ text: String) -> Double? {
        var s = text.trimmingCharacters(in: .whitespaces).lowercased()
        s = s.replacingOccurrences(of: "\u{2212}", with: "-")
        guard !s.isEmpty else { return nil }
        var factor = 1.0
        // Longest suffix first.
        for (suffix, f) in suffixes.sorted(by: { $0.0.count > $1.0.count }) where s.hasSuffix(suffix) {
            let head = String(s.dropLast(suffix.count))
            // "1e3m": only strip if what's left parses; keeps "1e3" intact for "e".
            if let v = ExpressionParser.evaluate(head) {
                return v * f
            }
        }
        guard let v = ExpressionParser.evaluate(s) else { return nil }
        factor = 1
        return v * factor
    }

    private var suffixes: [(String, Double)] {
        switch quantity {
        case .length: [("mm", 0.001), ("cm", 0.01), ("m", 1), ("ft", 0.3048), ("in", 0.0254), ("'", 0.3048), ("\"", 0.0254)]
        case .frequency: [("khz", 1000), ("hz", 1), ("k", 1000)]
        case .level: [("db", 1)]
        case .angle: [("°", 1), ("deg", 1), ("rad", 180 / .pi)]
        case .time: [("ms", 0.001), ("s", 1)]
        case .delay: [("ms", 1), ("s", 1000)]
        case .percent: [("%", 1)]
        case .temperature: [("°c", 1), ("c", 1)]
        case .ratio, .count, .plain: []
        }
    }
}

/// A small safe evaluator: numbers, + − × ÷ (also * /), parentheses, unary
/// minus, `pi`, and `sqrt`, `sin`, `cos` (degrees). Decimal comma accepted
/// when unambiguous.
public enum ExpressionParser {
    public static func evaluate(_ text: String) -> Double? {
        var s = text.replacingOccurrences(of: " ", with: "")
            .replacingOccurrences(of: "×", with: "*")
            .replacingOccurrences(of: "÷", with: "/")
            .replacingOccurrences(of: "\u{2212}", with: "-")
        // A single comma with no dot is a decimal separator.
        if !s.contains("."), s.filter({ $0 == "," }).count == 1 {
            s = s.replacingOccurrences(of: ",", with: ".")
        }
        var p = Parser(chars: Array(s.lowercased()))
        guard let v = p.expression(), p.i == p.chars.count, v.isFinite else { return nil }
        return v
    }

    private struct Parser {
        let chars: [Character]
        var i = 0

        mutating func peek() -> Character? { i < chars.count ? chars[i] : nil }

        mutating func expression() -> Double? {
            guard var v = term() else { return nil }
            while let c = peek(), c == "+" || c == "-" {
                i += 1
                guard let r = term() else { return nil }
                v = c == "+" ? v + r : v - r
            }
            return v
        }

        mutating func term() -> Double? {
            guard var v = factor() else { return nil }
            while let c = peek(), c == "*" || c == "/" {
                i += 1
                guard let r = factor() else { return nil }
                if c == "/" {
                    guard r != 0 else { return nil }
                    v /= r
                } else {
                    v *= r
                }
            }
            return v
        }

        mutating func factor() -> Double? {
            guard let c = peek() else { return nil }
            if c == "-" { i += 1; return factor().map { -$0 } }
            if c == "+" { i += 1; return factor() }
            if c == "(" {
                i += 1
                guard let v = expression(), peek() == ")" else { return nil }
                i += 1
                return v
            }
            if c.isLetter {
                var name = ""
                while let ch = peek(), ch.isLetter { name.append(ch); i += 1 }
                if name == "pi" { return .pi }
                guard ["sqrt", "sin", "cos"].contains(name), peek() == "(" else { return nil }
                guard let arg = factor() else { return nil }
                switch name {
                case "sqrt": return arg >= 0 ? arg.squareRoot() : nil
                case "sin": return sin(arg * .pi / 180)
                default: return cos(arg * .pi / 180)
                }
            }
            return number()
        }

        mutating func number() -> Double? {
            let start = i
            while let c = peek(), c.isNumber || c == "." { i += 1 }
            // Exponent: 1e3, 2.5e-2
            if let c = peek(), c == "e", i > start {
                let save = i
                i += 1
                if let s = peek(), s == "-" || s == "+" { i += 1 }
                let digits = i
                while let d = peek(), d.isNumber { i += 1 }
                if i == digits { i = save }
            }
            guard i > start else { return nil }
            return Double(String(chars[start..<i]))
        }
    }
}
