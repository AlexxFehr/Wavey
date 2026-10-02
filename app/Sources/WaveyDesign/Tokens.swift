import SwiftUI

/// Design tokens (dark theme). Feature code uses these only: no hex
/// literals or magic numbers outside this module.
public enum WD {}

public extension Color {
    /// `0xRRGGBB` in sRGB.
    init(rgb: UInt32, opacity: Double = 1) {
        self.init(.sRGB,
                  red: Double((rgb >> 16) & 0xff) / 255,
                  green: Double((rgb >> 8) & 0xff) / 255,
                  blue: Double(rgb & 0xff) / 255,
                  opacity: opacity)
    }

    /// `#rrggbb`; nil if malformed.
    init?(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("#") { s.removeFirst() }
        guard s.count == 6, let v = UInt32(s, radix: 16) else { return nil }
        self.init(rgb: v)
    }

    static let wd = WDColors()
}

public struct WDColors: Sendable {
    // Surfaces
    public let surfaceChrome = Color(rgb: 0x15171A)
    public let surfaceBar = Color(rgb: 0x17191C)
    public let surfacePanel = Color(rgb: 0x1C1E22)
    public let surfaceViewportPlan = Color(rgb: 0x121417)
    public let viewportCentre = Color(rgb: 0x171A1E)
    public let viewportEdge = Color(rgb: 0x0F1012)
    public let fieldFocused = Color(rgb: 0x202328)

    // Controls
    public let controlFill = Color(rgb: 0x272A30)
    public let controlHover = Color(rgb: 0x2C3037)
    public let chipHover = Color(rgb: 0x2F333A)
    public let buttonFill = Color(rgb: 0x2A2D33)
    public let buttonHover = Color(rgb: 0x32363D)
    public let switcherTrack = Color(rgb: 0x1D2024)
    public let switcherActive = Color(rgb: 0x2E3238)
    public let rowHover = Color(rgb: 0x24272C)
    public let hairline = Color(rgb: 0x2A2D33)
    public let rowDivider = Color(rgb: 0x23262B)

    // Text
    public let textPrimary = Color(rgb: 0xF1F4F7)
    public let textSecondary = Color(rgb: 0xD4D9DF)
    public let textTertiary = Color(rgb: 0x8A949E)
    public let textDisabled = Color(rgb: 0x5F6872)
    public let sectionTitle = Color(rgb: 0xC4CBD2)
    public let polarRing = Color(rgb: 0x6E7781)

    // Accent and status
    public let accent = Color(rgb: 0x4DA3FF)
    public let accentText = Color(rgb: 0x6DB3FF)
    public let accentTextOnFill = Color(rgb: 0xA8D2FF)
    public let accentFill = Color(rgb: 0x25405F)
    public let selectionRow = Color(rgb: 0x233B59)
    public let primaryButton = Color(rgb: 0x2B74D0)
    public let primaryButtonHover = Color(rgb: 0x3380E0)
    public let source = Color(rgb: 0xF29D38)
    public let sourceFill = Color(rgb: 0x2A1E10)
    public let receiver = Color(rgb: 0xE4E7EA)
    public let axisX = Color(rgb: 0xE06C5A)
    public let axisY = Color(rgb: 0x6CC07A)
    public let axisZ = Color(rgb: 0x4DA3FF)
    public let statusCurrent = Color(rgb: 0x6DB3FF)
    public let statusOutdated = Color(rgb: 0xF29D38)
    public let statusError = Color(rgb: 0xFF6B6B)

    // Viewport geometry
    public let planOutline = Color(rgb: 0xC9D1D9)
    public let volumeFloor = Color(rgb: 0x18222C)
    public let ghostWall = Color(rgb: 0x1F2B37)
    public let ghostEdge = Color(rgb: 0x3E4650)
    public let gridMinor = Color(rgb: 0x1C1F23)
    public let gridMajor = Color(rgb: 0x24282D)
    public let gridFloor3D = Color(rgb: 0x20252B)
    public let gridAxis = Color(rgb: 0x2C3238)
    public let plotGridMajor = Color(rgb: 0x30353B)
    public let plotGridMinor = Color(rgb: 0x22262B)
    public let plotGridY = Color(rgb: 0x262A30)
    public let legendEdge = Color(rgb: 0x2C3036)
    public let scattering = Color(rgb: 0x7FBFFF)
    public let absorption = Color(rgb: 0xD9A35B)
    public let curve250 = Color(rgb: 0x6E7781)
    public let curve4k = Color(rgb: 0xF2D2A8)

    /// Microphone / curve palette, distinct in lightness; cycle in order.
    public let curves: [Color] = [0x4DA3FF, 0xF29D38, 0xE4E7EA, 0xB48CF2, 0x6CC07A, 0xF2D2A8].map { Color(rgb: $0) }
    public let curveHex = ["#4da3ff", "#f29d38", "#e4e7ea", "#b48cf2", "#6cc07a", "#f2d2a8"]
    /// Default material swatches.
    public let swatches = ["#c9c2b4", "#d9a35b", "#9aa4ae", "#8c6a4a", "#b48cf2", "#5f6872",
                           "#4da3ff", "#6cc07a", "#e06c5a", "#f2d2a8", "#e4e7ea", "#7fbfff"]

    public func curve(_ i: Int) -> Color { curves[((i % curves.count) + curves.count) % curves.count] }
}

/// Metrics (points). Spacing scale is 2, 4, 6, 8, 10, 12.
public extension CGFloat {
    static let wd = WDMetrics()
}

public struct WDMetrics: Sendable {
    public let s2: CGFloat = 2, s4: CGFloat = 4, s6: CGFloat = 6, s8: CGFloat = 8, s10: CGFloat = 10, s12: CGFloat = 12

    public let topBar: CGFloat = 40
    public let statusBar: CGFloat = 22
    public let railWidth: CGFloat = 36
    public let railButton: CGFloat = 28
    public let railTopPadding: CGFloat = 8
    public let railSeparator: CGFloat = 18
    public let leftPanel: CGFloat = 240, leftPanelMin: CGFloat = 200, leftPanelMax: CGFloat = 360
    public let inspector: CGFloat = 300, inspectorMin: CGFloat = 260, inspectorMax: CGFloat = 420
    public let viewportHeader: CGFloat = 30
    public let dockTabBar: CGFloat = 30
    public let sectionHeader: CGFloat = 30
    public let row: CGFloat = 24
    public let rowIndent: CGFloat = 16
    public let propertyRow: CGFloat = 26
    public let propertyLabel: CGFloat = 76
    public let control: CGFloat = 22
    public let chip: CGFloat = 20
    public let button: CGFloat = 24
    public let iconSmall: CGFloat = 20
    public let iconRegular: CGFloat = 28
    public let segment: CGFloat = 28
    public let trafficLights: CGFloat = 78

    public let radiusField: CGFloat = 4
    public let radiusButton: CGFloat = 5
    public let radiusSegment: CGFloat = 6
    public let radiusTrack: CGFloat = 8
    public let radiusPopover: CGFloat = 8

    public let hairlineWidth: CGFloat = 1

    /// Overlay card opacity.
    public static let overlayAlpha: Double = 0.92
    /// Hover/press fill transition (s).
    public static let fillDuration: Double = 0.1
}
