import Foundation
import SwiftUI

/// The custom icon set: 16 × 16 grid, 1.4 pt stroke, round caps and joins,
/// no fills (dots excepted), drawn in the current foreground colour.
public enum WDIconName: String, CaseIterable, Sendable {
    case select, move, rotate, line, polyline, rectangle, arc, offset, extrude, surface, split, measure, snap
    case eye, eyeOff = "eye-off", lock, unlock
    case speaker, pointSource = "point-source", subwoofer, microphone, receiverGrid = "receiver-grid", aim
    case cutPlane = "cut-plane", probeLine = "probe-line"
    case play, stop, chevronDown = "chevron-down", chevronRight = "chevron-right", plus, minus, close
    case layers, undo, redo, cube, topView = "top-view", check, more, fit, waveform, solve, chart, link, filter
    case search, warning, gear, image, trash, sidebarLeft = "sidebar-left", sidebarRight = "sidebar-right", dock
    case stack, lineArray = "line-array"
}

/// One icon's strokes or dots, drawn in a 16-unit box scaled to the frame.
struct IconShape: Shape {
    let name: WDIconName
    let dots: Bool
    nonisolated(unsafe) private static var cache: [String: (Path, Path)] = [:]
    private static let lock = NSLock()

    func path(in rect: CGRect) -> Path {
        Self.lock.lock()
        let pair: (Path, Path)
        if let c = Self.cache[name.rawValue] { pair = c } else { pair = WDIcon.paths(name); Self.cache[name.rawValue] = pair }
        Self.lock.unlock()
        let s = rect.width / 16
        return (dots ? pair.1 : pair.0).applying(CGAffineTransform(scaleX: s, y: s).translatedBy(x: rect.minX / s, y: rect.minY / s))
    }
}

public struct WDIcon: View {
    let name: WDIconName
    let size: CGFloat

    /// `size` 16 (regular) or 12 (inline; drawn with a slightly heavier stroke).
    public init(_ name: WDIconName, size: CGFloat = 16) {
        self.name = name
        self.size = size
    }

    /// Shapes rather than a Canvas: icons sit in every row and button, and
    /// a Canvas per icon made each list update cost milliseconds.
    public var body: some View {
        let stroke = (size <= 12 ? 1.5 : 1.4) * 16 / size * (size / 16)
        ZStack {
            IconShape(name: name, dots: false).stroke(style: StrokeStyle(lineWidth: stroke, lineCap: .round, lineJoin: .round))
            IconShape(name: name, dots: true).fill()
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }

    // swiftlint:disable:next cyclomatic_complexity function_body_length
    static func paths(_ name: WDIconName) -> (Path, Path) {
        var p = Path()
        var d = Path()
        func l(_ pts: [(CGFloat, CGFloat)], close: Bool = false) {
            guard let f = pts.first else { return }
            p.move(to: CGPoint(x: f.0, y: f.1))
            for q in pts.dropFirst() { p.addLine(to: CGPoint(x: q.0, y: q.1)) }
            if close { p.closeSubpath() }
        }
        func rect(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat, r: CGFloat = 1) {
            p.addRoundedRect(in: CGRect(x: x, y: y, width: w, height: h), cornerSize: CGSize(width: r, height: r))
        }
        func circle(_ x: CGFloat, _ y: CGFloat, _ r: CGFloat) {
            p.addEllipse(in: CGRect(x: x - r, y: y - r, width: 2 * r, height: 2 * r))
        }
        func dot(_ x: CGFloat, _ y: CGFloat, _ r: CGFloat = 1) {
            d.addEllipse(in: CGRect(x: x - r, y: y - r, width: 2 * r, height: 2 * r))
        }
        func arc(_ x: CGFloat, _ y: CGFloat, _ r: CGFloat, _ a0: Double, _ a1: Double) {
            p.addArc(center: CGPoint(x: x, y: y), radius: r, startAngle: .degrees(a0), endAngle: .degrees(a1), clockwise: false)
        }

        switch name {
        case .select:
            l([(4, 2.5), (4, 13), (6.8, 10.4), (8.6, 14), (10.2, 13.2), (8.4, 9.7), (12, 9.4)], close: true)
        case .rotate:
            arc(8, 8, 5.5, 200, 520)
            l([(13.6, 3.6), (13.3, 7.0), (10.0, 6.3)])
        case .move:
            l([(8, 1.8), (8, 14.2)]); l([(1.8, 8), (14.2, 8)])
            l([(6.2, 3.6), (8, 1.8), (9.8, 3.6)]); l([(6.2, 12.4), (8, 14.2), (9.8, 12.4)])
            l([(3.6, 6.2), (1.8, 8), (3.6, 9.8)]); l([(12.4, 6.2), (14.2, 8), (12.4, 9.8)])
        case .line:
            l([(3, 13), (13, 3)]); dot(3, 13, 1.3); dot(13, 3, 1.3)
        case .polyline:
            l([(2.5, 12.5), (6, 5), (10, 10), (13.5, 3.5)]); dot(2.5, 12.5, 1.2); dot(13.5, 3.5, 1.2)
        case .rectangle:
            rect(2.5, 4, 11, 8, r: 0.8)
        case .arc:
            arc(8, 12, 6, 180, 360); dot(2, 12, 1.2); dot(14, 12, 1.2)
        case .offset:
            l([(2.5, 13.5), (2.5, 5.5), (10.5, 5.5)]); l([(5.5, 13.5), (5.5, 8.5), (13.5, 8.5)])
            l([(11, 2.5), (13.5, 2.5)])
        case .extrude:
            l([(2.5, 13), (9.5, 13), (13.5, 10), (6.5, 10)], close: true)
            l([(8, 9), (8, 2.5)]); l([(6, 4.5), (8, 2.5), (10, 4.5)])
        case .surface:
            l([(2, 11.5), (5.5, 4.5), (14, 4.5), (10.5, 11.5)], close: true)
        case .split:
            l([(2, 11.5), (5.5, 4.5), (14, 4.5), (10.5, 11.5)], close: true)
            l([(8.5, 2), (7.5, 14)])
        case .measure:
            l([(2, 10.5), (10.5, 2), (14, 5.5), (5.5, 14)], close: true)
            l([(5, 7.5), (6.5, 9)]); l([(7.5, 5), (9, 6.5)]); l([(10, 2.5 + 2.5), (11.5, 6.5)])
        case .snap:
            l([(4, 2.5), (4, 8), (12, 8), (12, 2.5)]); l([(4, 8), (4, 9)])
            arc(8, 9, 4, 0, 180); l([(2.5, 2.5), (5.5, 2.5)]); l([(10.5, 2.5), (13.5, 2.5)])
        case .eye:
            p.move(to: CGPoint(x: 1.5, y: 8)); p.addQuadCurve(to: CGPoint(x: 14.5, y: 8), control: CGPoint(x: 8, y: 0.5))
            p.addQuadCurve(to: CGPoint(x: 1.5, y: 8), control: CGPoint(x: 8, y: 15.5)); circle(8, 8, 2.2)
        case .eyeOff:
            p.move(to: CGPoint(x: 1.5, y: 8)); p.addQuadCurve(to: CGPoint(x: 14.5, y: 8), control: CGPoint(x: 8, y: 0.5))
            p.addQuadCurve(to: CGPoint(x: 1.5, y: 8), control: CGPoint(x: 8, y: 15.5)); l([(2.5, 13.5), (13.5, 2.5)])
        case .lock:
            rect(3, 7, 10, 7); p.move(to: CGPoint(x: 5, y: 7)); p.addLine(to: CGPoint(x: 5, y: 5))
            arc(8, 5, 3, 180, 360); p.addLine(to: CGPoint(x: 11, y: 7))
        case .unlock:
            rect(3, 7, 10, 7); p.move(to: CGPoint(x: 5, y: 7)); p.addLine(to: CGPoint(x: 5, y: 5))
            arc(8, 5, 3, 180, 330)
        case .speaker:
            rect(4, 1.8, 8, 12.4, r: 1.2); circle(8, 10, 2.2); dot(8, 4.6, 0.9)
        case .pointSource:
            dot(8, 8, 1.6); arc(8, 8, 4, 0, 360)
            arc(8, 8, 6.4, -40, 40); arc(8, 8, 6.4, 140, 220)
        case .subwoofer:
            rect(2.5, 2.5, 11, 11, r: 1.2); circle(8, 8, 3.6); dot(8, 8, 1)
        case .microphone:
            rect(6, 1.8, 4, 7.4, r: 2); arc(8, 7.5, 4, 0, 180); l([(8, 11.5), (8, 14)]); l([(6, 14), (10, 14)])
        case .receiverGrid:
            for x in [3.5, 8.0, 12.5] { for y in [3.5, 8.0, 12.5] { circle(x, y, 1.3) } }
        case .aim:
            circle(8, 8, 5); dot(8, 8, 1.1)
            l([(8, 1.5), (8, 4)]); l([(8, 12), (8, 14.5)]); l([(1.5, 8), (4, 8)]); l([(12, 8), (14.5, 8)])
        case .cutPlane:
            l([(1.5, 11), (5, 5), (14.5, 5), (11, 11)], close: true); l([(8, 8), (8, 1.5)]); l([(6.3, 3.2), (8, 1.5), (9.7, 3.2)])
        case .probeLine:
            l([(2.5, 13.5), (13.5, 2.5)]); for t in [0.0, 0.33, 0.66, 1.0] { dot(2.5 + 11 * t, 13.5 - 11 * t, 1.2) }
        case .play:
            l([(4.5, 2.5), (13, 8), (4.5, 13.5)], close: true)
        case .stop:
            rect(3.5, 3.5, 9, 9, r: 1.2)
        case .chevronDown:
            l([(4, 6), (8, 10), (12, 6)])
        case .chevronRight:
            l([(6, 4), (10, 8), (6, 12)])
        case .plus:
            l([(8, 3), (8, 13)]); l([(3, 8), (13, 8)])
        case .minus:
            l([(3, 8), (13, 8)])
        case .close:
            l([(4, 4), (12, 12)]); l([(12, 4), (4, 12)])
        case .layers:
            l([(8, 2), (14.5, 5.5), (8, 9), (1.5, 5.5)], close: true)
            l([(1.5, 8.5), (8, 12), (14.5, 8.5)])
        case .undo:
            l([(5.5, 3), (2.5, 6), (5.5, 9)]); p.move(to: CGPoint(x: 2.5, y: 6)); p.addLine(to: CGPoint(x: 10, y: 6))
            arc(10, 9.25, 3.25, -90, 90); p.addLine(to: CGPoint(x: 6, y: 12.5))
        case .redo:
            l([(10.5, 3), (13.5, 6), (10.5, 9)]); p.move(to: CGPoint(x: 13.5, y: 6)); p.addLine(to: CGPoint(x: 6, y: 6))
            p.addArc(center: CGPoint(x: 6, y: 9.25), radius: 3.25, startAngle: .degrees(270), endAngle: .degrees(90), clockwise: true)
            p.addLine(to: CGPoint(x: 10, y: 12.5))
        case .cube:
            l([(8, 1.8), (13.5, 4.8), (13.5, 11.2), (8, 14.2), (2.5, 11.2), (2.5, 4.8)], close: true)
            l([(2.5, 4.8), (8, 7.8), (13.5, 4.8)]); l([(8, 7.8), (8, 14.2)])
        case .topView:
            rect(2.5, 2.5, 11, 11, r: 1); l([(2.5, 6.5), (13.5, 6.5)]); dot(8, 10, 1)
        case .check:
            l([(3, 8.5), (6.5, 12), (13, 4.5)])
        case .more:
            dot(3.5, 8, 1.2); dot(8, 8, 1.2); dot(12.5, 8, 1.2)
        case .fit:
            l([(2, 5.5), (2, 2), (5.5, 2)]); l([(10.5, 2), (14, 2), (14, 5.5)])
            l([(14, 10.5), (14, 14), (10.5, 14)]); l([(5.5, 14), (2, 14), (2, 10.5)]); rect(5.5, 5.5, 5, 5, r: 0.6)
        case .waveform:
            l([(1.5, 8), (3.5, 8), (5, 3), (7, 13), (9, 4.5), (11, 11), (12.5, 8), (14.5, 8)])
        case .solve:
            l([(4.5, 2.5), (13, 8), (4.5, 13.5)], close: true)
        case .chart:
            l([(2, 2), (2, 14), (14, 14)]); l([(4, 10.5), (7, 6.5), (9.5, 9), (13.5, 3.5)])
        case .link:
            p.addRoundedRect(in: CGRect(x: 1.5, y: 5.5, width: 7.5, height: 5), cornerSize: CGSize(width: 2.5, height: 2.5))
            p.addRoundedRect(in: CGRect(x: 7, y: 5.5, width: 7.5, height: 5), cornerSize: CGSize(width: 2.5, height: 2.5))
        case .filter:
            l([(2, 3), (14, 3), (9.5, 8.5), (9.5, 13), (6.5, 14), (6.5, 8.5)], close: true)
        case .search:
            circle(7, 7, 4.5); l([(10.3, 10.3), (14, 14)])
        case .warning:
            l([(8, 2), (14.5, 13.5), (1.5, 13.5)], close: true); l([(8, 6.5), (8, 9.5)]); dot(8, 11.6, 0.9)
        case .gear:
            circle(8, 8, 2.3)
            for i in 0..<8 {
                let a = Double(i) * .pi / 4
                l([(8 + 4 * CGFloat(cos(a)), 8 + 4 * CGFloat(sin(a))), (8 + 6.2 * CGFloat(cos(a)), 8 + 6.2 * CGFloat(sin(a)))])
            }
            circle(8, 8, 4.2)
        case .image:
            rect(2, 3, 12, 10, r: 1); l([(2.5, 12), (6.5, 8), (9, 10.5), (11, 8.5), (13.5, 11)]); dot(10.5, 5.8, 1)
        case .trash:
            l([(2.5, 4), (13.5, 4)]); l([(6, 4), (6.5, 2), (9.5, 2), (10, 4)])
            l([(4, 4), (4.8, 14), (11.2, 14), (12, 4)])
        case .sidebarLeft:
            rect(2, 3, 12, 10, r: 1.2); l([(6, 3), (6, 13)])
        case .sidebarRight:
            rect(2, 3, 12, 10, r: 1.2); l([(10, 3), (10, 13)])
        case .dock:
            rect(2, 3, 12, 10, r: 1.2); l([(2, 9.5), (14, 9.5)])
        case .stack:
            rect(3, 1.8, 10, 4, r: 1); rect(3, 6, 10, 4, r: 1); rect(3, 10.2, 10, 4, r: 1)
            dot(8, 3.8, 0.9); dot(8, 8, 0.9); dot(8, 12.2, 0.9)
        case .lineArray:
            l([(4, 2), (12, 2)])
            l([(4.5, 3.5), (11.5, 3.5), (11.2, 6), (4.8, 6)], close: true)
            l([(5, 7.2), (11, 7.6), (10.2, 10), (4.6, 9.4)], close: true)
            l([(4.4, 10.8), (9.8, 11.6), (8.4, 13.8), (3.6, 12.8)], close: true)
        }
        return (p, d)
    }
}
