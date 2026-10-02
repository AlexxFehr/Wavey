import SwiftUI

// MARK: Buttons

public enum WDIconButtonSize: Sendable { case regular, small }

/// Icon-only button: 28 pt (regular) or 20 pt (small).
public struct WDIconButton: View {
    let icon: WDIconName
    let size: WDIconButtonSize
    let isOn: Bool
    let help: String
    let action: () -> Void
    @Environment(\.isEnabled) private var enabled
    @State private var hover = false

    public init(_ icon: WDIconName, size: WDIconButtonSize = .regular, isOn: Bool = false, help: String, action: @escaping () -> Void) {
        self.icon = icon
        self.size = size
        self.isOn = isOn
        self.help = help
        self.action = action
    }

    public var body: some View {
        let side: CGFloat = size == .regular ? .wd.iconRegular : .wd.iconSmall
        Button(action: action) {
            WDIcon(icon, size: size == .regular ? 16 : 12)
                .foregroundStyle(!enabled ? Color.wd.textDisabled : isOn ? Color.wd.accentText : hover ? Color.wd.textPrimary : Color.wd.textTertiary)
                .frame(width: side, height: side)
                .background(
                    RoundedRectangle(cornerRadius: size == .regular ? .wd.radiusSegment : .wd.radiusField)
                        .fill(isOn ? Color.wd.accentFill : hover && enabled ? Color.wd.controlFill : .clear)
                )
                .contentShape(Rectangle().inset(by: size == .small ? -2 : 0))
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
        .animation(.easeOut(duration: WDMetrics.fillDuration), value: hover)
        .help(help)
        .accessibilityLabel(help)
        .accessibilityAddTraits(isOn ? [.isSelected] : [])
    }
}

/// Secondary text button (24 pt).
public struct WDButtonStyle: ButtonStyle {
    let primary: Bool
    public init(primary: Bool = false) { self.primary = primary }

    public func makeBody(configuration: Configuration) -> some View {
        WDButtonBody(configuration: configuration, primary: primary)
    }

    struct WDButtonBody: View {
        let configuration: ButtonStyle.Configuration
        let primary: Bool
        @State private var hover = false
        @Environment(\.isEnabled) private var enabled

        var body: some View {
            let base = primary ? Color.wd.primaryButton : Color.wd.buttonFill
            let over = primary ? Color.wd.primaryButtonHover : Color.wd.buttonHover
            configuration.label
                .font(.wd.button)
                .foregroundStyle(enabled ? (primary ? Color.white : Color.wd.textSecondary) : Color.wd.textDisabled)
                .padding(.horizontal, 11)
                .frame(minHeight: .wd.button)
                .background(RoundedRectangle(cornerRadius: .wd.radiusButton).fill(configuration.isPressed ? base.opacity(0.85) : hover && enabled ? over : base))
                .onHover { hover = $0 }
                .animation(.easeOut(duration: WDMetrics.fillDuration), value: hover)
        }
    }
}

public extension ButtonStyle where Self == WDButtonStyle {
    static var wd: WDButtonStyle { WDButtonStyle() }
    static var wdPrimary: WDButtonStyle { WDButtonStyle(primary: true) }
}

/// Text-only link-style button.
public struct WDTextButton: View {
    let title: String
    let action: () -> Void
    @State private var hover = false
    public init(_ title: String, action: @escaping () -> Void) {
        self.title = title
        self.action = action
    }
    public var body: some View {
        Button(action: action) {
            Text(title).font(.wd.body).foregroundStyle(hover ? Color.wd.accentTextOnFill : Color.wd.accentText)
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
    }
}

// MARK: Chips

/// Toggle chip (20 pt, mono 10.5).
public struct WDChip: View {
    let title: String
    let icon: WDIconName?
    let isOn: Bool
    let action: () -> Void
    @State private var hover = false
    @Environment(\.isEnabled) private var enabled

    public init(_ title: String, icon: WDIconName? = nil, isOn: Bool, action: @escaping () -> Void) {
        self.title = title
        self.icon = icon
        self.isOn = isOn
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: .wd.s4) {
                if let icon { WDIcon(icon, size: 12) }
                Text(title).font(.wd.chip).lineLimit(1).fixedSize()
            }
            .foregroundStyle(!enabled ? Color.wd.textDisabled : isOn ? Color.wd.accentTextOnFill : Color.wd.textSecondary)
            .padding(.horizontal, .wd.s8)
            .frame(height: .wd.chip)
            .background(RoundedRectangle(cornerRadius: .wd.radiusField)
                .fill(isOn ? Color.wd.accentFill : hover && enabled ? Color.wd.chipHover : Color.wd.controlFill))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
        .animation(.easeOut(duration: WDMetrics.fillDuration), value: hover)
        .accessibilityAddTraits(.isToggle)
        .accessibilityValue(isOn ? "on" : "off")
    }
}

/// Mutually exclusive chips.
public struct WDChipGroup<T: Hashable>: View {
    let options: [(T, String)]
    @Binding var selection: T
    public init(_ options: [(T, String)], selection: Binding<T>) {
        self.options = options
        self._selection = selection
    }
    public var body: some View {
        HStack(spacing: .wd.s4) {
            ForEach(options, id: \.0) { opt in
                WDChip(opt.1, isOn: selection == opt.0) { selection = opt.0 }
            }
        }
    }
}

/// Full-width segmented control for 2–4 options inside inspectors.
public struct WDSegmented<T: Hashable>: View {
    let options: [(T, String)]
    let selection: T?
    let onSelect: (T) -> Void
    public init(_ options: [(T, String)], selection: T?, onSelect: @escaping (T) -> Void) {
        self.options = options
        self.selection = selection
        self.onSelect = onSelect
    }
    public var body: some View {
        HStack(spacing: .wd.s4) {
            ForEach(options, id: \.0) { opt in
                Button { onSelect(opt.0) } label: {
                    Text(opt.1).font(.wd.chip).lineLimit(1)
                        .foregroundStyle(selection == opt.0 ? Color.wd.accentTextOnFill : Color.wd.textSecondary)
                        .frame(maxWidth: .infinity, minHeight: .wd.control)
                        .background(RoundedRectangle(cornerRadius: .wd.radiusField)
                            .fill(selection == opt.0 ? Color.wd.accentFill : Color.wd.controlFill))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(selection == opt.0 ? [.isSelected] : [])
            }
        }
    }
}

/// 14 pt checkbox with optional label.
public struct WDCheckbox: View {
    let label: String?
    let isOn: Bool?
    let onToggle: (Bool) -> Void
    /// `isOn` nil = mixed.
    public init(_ label: String? = nil, isOn: Bool?, onToggle: @escaping (Bool) -> Void) {
        self.label = label
        self.isOn = isOn
        self.onToggle = onToggle
    }
    public var body: some View {
        Button { onToggle(!(isOn ?? false)) } label: {
            HStack(spacing: .wd.s6) {
                ZStack {
                    RoundedRectangle(cornerRadius: 3).fill(isOn == true ? Color.wd.accent : Color.wd.controlFill)
                    if isOn == true {
                        WDIcon(.check, size: 12).foregroundStyle(Color.wd.surfaceChrome).scaleEffect(0.85)
                    } else if isOn == nil {
                        Rectangle().fill(Color.wd.textSecondary).frame(width: 7, height: 1.5)
                    }
                }
                .frame(width: 14, height: 14)
                if let label { Text(label).font(.wd.body).foregroundStyle(Color.wd.textSecondary) }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label ?? "")
        .accessibilityValue(isOn == true ? "on" : isOn == nil ? "mixed" : "off")
    }
}

// MARK: Layout

/// Section header (30 pt): title, then trailing content (counts, icon buttons).
public struct WDSectionHeader<Trailing: View>: View {
    let title: String
    let collapsed: Bool?
    let onToggle: (() -> Void)?
    let trailing: Trailing

    public init(_ title: String, collapsed: Bool? = nil, onToggle: (() -> Void)? = nil, @ViewBuilder trailing: () -> Trailing) {
        self.title = title
        self.collapsed = collapsed
        self.onToggle = onToggle
        self.trailing = trailing()
    }

    public var body: some View {
        HStack(spacing: .wd.s6) {
            Text(title)
                .font(.wd.section)
                .foregroundStyle(collapsed == true ? Color.wd.textTertiary : Color.wd.sectionTitle)
                .lineLimit(1)
                .contentShape(Rectangle())
                .onTapGesture { onToggle?() }
            Spacer(minLength: .wd.s4)
            trailing
        }
        .padding(.leading, .wd.s12)
        .padding(.trailing, .wd.s8)
        .frame(height: .wd.sectionHeader)
        .padding(.top, .wd.s2)
        .accessibilityElement(children: .contain)
        .accessibilityAddTraits(.isHeader)
    }
}

public extension WDSectionHeader where Trailing == EmptyView {
    init(_ title: String, collapsed: Bool? = nil, onToggle: (() -> Void)? = nil) {
        self.init(title, collapsed: collapsed, onToggle: onToggle) { EmptyView() }
    }
}

/// Mono count shown in section headers.
public struct WDCount: View {
    let text: String
    public init(_ n: Int) { text = "\(n)" }
    public init(_ text: String) { self.text = text }
    public var body: some View {
        Text(text).font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
    }
}

/// Property row: 76 pt label + control column.
public struct WDPropertyRow<Content: View>: View {
    let label: String
    let content: Content
    public init(_ label: String, @ViewBuilder content: () -> Content) {
        self.label = label
        self.content = content()
    }
    public var body: some View {
        HStack(alignment: .center, spacing: .wd.s8) {
            Text(label)
                .font(.wd.label)
                .foregroundStyle(Color.wd.textTertiary)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(width: .wd.propertyLabel, alignment: .leading)
                .help(label)
            content.frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, .wd.s12)
        .frame(minHeight: .wd.propertyRow)
    }
}

/// Read-only mono value in a property row.
public struct WDValueText: View {
    let text: String
    let color: Color
    public init(_ text: String, color: Color = .wd.textSecondary) {
        self.text = text
        self.color = color
    }
    public var body: some View {
        Text(text).font(.wd.numeric).foregroundStyle(color).lineLimit(1).textSelection(.enabled)
    }
}

/// Mono label/value pair for the status bar.
public struct WDStatusItem: View {
    let label: String?
    let value: String
    let dot: Color?
    let hollowDot: Bool
    public init(_ label: String? = nil, _ value: String, dot: Color? = nil, hollowDot: Bool = false) {
        self.label = label
        self.value = value
        self.dot = dot
        self.hollowDot = hollowDot
    }
    public var body: some View {
        HStack(spacing: .wd.s4) {
            if let dot {
                Circle()
                    .strokeBorder(dot, lineWidth: hollowDot ? 1 : 0)
                    .background(Circle().fill(hollowDot ? .clear : dot))
                    .frame(width: 6, height: 6)
            }
            if let label { Text(label).foregroundStyle(Color.wd.textTertiary) }
            Text(value).foregroundStyle(Color.wd.textSecondary)
        }
        .font(.wd.status)
        .lineLimit(1)
        .fixedSize()
    }
}

/// Text-only empty state.
public struct WDEmptyState: View {
    let title: String
    let detail: String?
    let actionTitle: String?
    let action: (() -> Void)?
    public init(_ title: String, detail: String? = nil, actionTitle: String? = nil, action: (() -> Void)? = nil) {
        self.title = title
        self.detail = detail
        self.actionTitle = actionTitle
        self.action = action
    }
    public var body: some View {
        VStack(spacing: .wd.s4) {
            Text(title).font(.wd.body).foregroundStyle(Color.wd.textSecondary)
            if let detail {
                Text(detail).font(.wd.detail).foregroundStyle(Color.wd.textTertiary).multilineTextAlignment(.center)
            }
            if let actionTitle, let action { WDTextButton(actionTitle, action: action).padding(.top, .wd.s2) }
        }
        .padding(.wd.s12)
    }
}

/// 1 pt vertical hairline (14 pt tall) between header groups.
public struct WDHairline: View {
    let height: CGFloat
    public init(height: CGFloat = 14) { self.height = height }
    public var body: some View {
        Rectangle().fill(Color.wd.hairline).frame(width: 1, height: height)
    }
}

// MARK: Slider

/// 2 pt track, accent fill, 12 pt thumb. Arrow keys step (⇧ ×10).
public struct WDSlider: View {
    let value: Double
    let range: ClosedRange<Double>
    let step: Double
    /// Maps value ↔ [0, 1] (identity for linear; log for frequency).
    let toUnit: (Double) -> Double
    let fromUnit: (Double) -> Double
    let onChange: (Double, Bool) -> Void
    @State private var hover = false

    /// `onChange(value, finished)`.
    public init(value: Double, range: ClosedRange<Double>, step: Double = 0, logarithmic: Bool = false,
                onChange: @escaping (Double, Bool) -> Void) {
        self.value = value
        self.range = range
        self.step = step
        self.onChange = onChange
        let (lo, hi) = (range.lowerBound, range.upperBound)
        if logarithmic, lo > 0 {
            toUnit = { log($0 / lo) / log(hi / lo) }
            fromUnit = { lo * pow(hi / lo, $0) }
        } else {
            toUnit = { ($0 - lo) / (hi - lo) }
            fromUnit = { lo + $0 * (hi - lo) }
        }
    }

    func snapped(_ v: Double) -> Double {
        let c = min(max(v, range.lowerBound), range.upperBound)
        return step > 0 ? (c / step).rounded() * step : c
    }

    public var body: some View {
        GeometryReader { geo in
            let w = geo.size.width - 12
            let t = CGFloat(min(max(toUnit(value), 0), 1))
            ZStack(alignment: .leading) {
                Capsule().fill(Color.wd.controlFill).frame(height: 2).padding(.horizontal, 6)
                Capsule().fill(Color.wd.accent).frame(width: max(0, t * w), height: 2).padding(.leading, 6)
                Circle().fill(Color.wd.textPrimary)
                    .frame(width: hover ? 14 : 12, height: hover ? 14 : 12)
                    .offset(x: t * w + 6 - (hover ? 7 : 6))
            }
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { g in onChange(snapped(fromUnit(Double(min(max((g.location.x - 6) / w, 0), 1)))), false) }
                .onEnded { g in onChange(snapped(fromUnit(Double(min(max((g.location.x - 6) / w, 0), 1)))), true) })
            .onHover { hover = $0 }
        }
        .frame(height: .wd.control)
        .focusable()
        .onKeyPress(.leftArrow) { nudge(-1); return .handled }
        .onKeyPress(.rightArrow) { nudge(1); return .handled }
        .accessibilityElement()
        .accessibilityValue(String(format: "%.2f", value))
        .accessibilityAdjustableAction { dir in nudge(dir == .increment ? 1 : -1) }
    }

    private func nudge(_ sign: Double) {
        let big = NSEvent.modifierFlags.contains(.shift) ? 10.0 : 1.0
        let s = step > 0 ? step : (range.upperBound - range.lowerBound) / 100
        onChange(snapped(value + sign * s * big), true)
    }
}

// MARK: Popover chrome

public extension View {
    /// Popover surface: panel fill, hairline edge, 8 radius, the app's only shadow.
    func wdPopoverChrome() -> some View {
        self.padding(.wd.s12)
            .background(RoundedRectangle(cornerRadius: .wd.radiusPopover).fill(Color.wd.surfacePanel))
            .overlay(RoundedRectangle(cornerRadius: .wd.radiusPopover).strokeBorder(Color.wd.hairline, lineWidth: 1))
            .shadow(color: .black.opacity(0.45), radius: 12, y: 8)
    }

    /// Viewport overlay card: chrome at 92%, hairline edge, 6 radius.
    func wdOverlayCard() -> some View {
        self.background(RoundedRectangle(cornerRadius: .wd.radiusSegment).fill(Color.wd.surfaceChrome.opacity(WDMetrics.overlayAlpha)))
            .overlay(RoundedRectangle(cornerRadius: .wd.radiusSegment).strokeBorder(Color.wd.hairline, lineWidth: 1))
    }
}
