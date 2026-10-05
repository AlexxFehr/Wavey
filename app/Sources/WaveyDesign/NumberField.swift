import AppKit
import SwiftUI

/// Numeric field: mono, right-aligned, unit suffix, expression and unit
/// parsing, ↑/↓ stepping, Esc revert, select-all on focus, mixed "—" state.
/// Commits on Enter, Tab or focus loss.
public struct WDNumberField: View {
    let value: Double?
    let formatter: UnitFormatter
    let range: ClosedRange<Double>?
    let step: Double
    let showUnit: Bool
    let unitOverride: String?
    let onCommit: (Double) -> Void
    @State private var focused = false
    @State private var editing = false
    @State private var invalid: String?
    @State private var hover = false
    @Environment(\.isEnabled) private var enabled

    /// `value` nil shows "—" (mixed multi-selection); typing replaces all.
    public init(_ value: Double?, _ formatter: UnitFormatter, range: ClosedRange<Double>? = nil, step: Double? = nil,
                showUnit: Bool = true, unit: String? = nil, onCommit: @escaping (Double) -> Void) {
        self.value = value
        self.formatter = formatter
        self.range = range
        self.step = step ?? pow(10, -Double(formatter.decimals))
        self.showUnit = showUnit
        self.unitOverride = unit
        self.onCommit = onCommit
    }

    public var body: some View {
        let unit = unitOverride ?? formatter.unit
        HStack(spacing: .wd.s4) {
            // Plain text until clicked: an AppKit text field per number made
            // every inspector change (a new selection) cost milliseconds.
            if editing || focused {
                NumberTextField(value: value, formatter: formatter, range: range, step: step,
                                enabled: enabled, focused: $focused, invalid: $invalid, autoFocus: true, onCommit: onCommit)
            } else {
                Text(value.map { formatter.string($0) } ?? "—")
                    .font(.wd.numeric)
                    .foregroundStyle(enabled ? Color.wd.textSecondary : Color.wd.textDisabled)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
            if showUnit, !unit.isEmpty, unitOverride != nil || !focused {
                Text(unit).font(.wd.numeric).foregroundStyle(Color.wd.textTertiary).fixedSize()
            }
        }
        .padding(.horizontal, 7)
        .frame(height: .wd.control)
        .background(RoundedRectangle(cornerRadius: .wd.radiusField)
            .fill(focused ? Color.wd.fieldFocused : hover && enabled ? Color.wd.controlHover : Color.wd.controlFill))
        .overlay(RoundedRectangle(cornerRadius: .wd.radiusField)
            .strokeBorder(invalid != nil ? Color.wd.statusError : focused ? Color.wd.accent : .clear, lineWidth: 1))
        .contentShape(Rectangle())
        .onTapGesture { if enabled { editing = true } }
        .onChange(of: focused) { _, on in
            // After the field has committed its text.
            if !on { DispatchQueue.main.async { if !focused { editing = false } } }
        }
        .onHover { hover = $0 }
        .help(invalid ?? "")
        .accessibilityElement(children: .combine)
        .accessibilityValue(value.map { formatter.withUnit($0) } ?? "mixed")
    }
}

/// Scrubbable label: drag horizontally to change a value (step per 4 pt;
/// ⇧ ×10, ⌥ ×0.1). `onScrub(value, finished)` fires live.
public struct WDScrubLabel: View {
    let text: String
    let value: Double?
    let step: Double
    let range: ClosedRange<Double>?
    let onScrub: (Double, Bool) -> Void
    @State private var start: Double?

    public init(_ text: String, value: Double?, step: Double, range: ClosedRange<Double>? = nil, onScrub: @escaping (Double, Bool) -> Void) {
        self.text = text
        self.value = value
        self.step = step
        self.range = range
        self.onScrub = onScrub
    }

    public var body: some View {
        Text(text)
            .font(.wd.label)
            .foregroundStyle(Color.wd.textTertiary)
            .lineLimit(1)
            .onHover { inside in
                if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() }
            }
            .gesture(DragGesture(minimumDistance: 2)
                .onChanged { g in
                    guard let v0 = start ?? value else { return }
                    if start == nil { start = v0 }
                    onScrub(scrubbed(v0, g.translation.width), false)
                }
                .onEnded { g in
                    if let v0 = start { onScrub(scrubbed(v0, g.translation.width), true) }
                    start = nil
                })
    }

    private func scrubbed(_ v0: Double, _ dx: CGFloat) -> Double {
        let f = NSEvent.modifierFlags
        let mult = f.contains(.shift) ? 10 : f.contains(.option) ? 0.1 : 1
        var v = v0 + (Double(dx) / 4).rounded() * step * mult
        if let range { v = min(max(v, range.lowerBound), range.upperBound) }
        return v
    }
}

/// Three numeric fields in equal columns (X · Y · Z, Az · El · Roll, …).
public struct WDVec3Field: View {
    let values: [Double?]
    let formatter: UnitFormatter
    let ranges: [ClosedRange<Double>?]
    let onCommit: (Int, Double) -> Void

    public init(_ values: [Double?], _ formatter: UnitFormatter, ranges: [ClosedRange<Double>?] = [nil, nil, nil],
                onCommit: @escaping (Int, Double) -> Void) {
        self.values = values
        self.formatter = formatter
        self.ranges = ranges
        self.onCommit = onCommit
    }

    public var body: some View {
        HStack(spacing: .wd.s4) {
            ForEach(0..<values.count, id: \.self) { i in
                WDNumberField(values[i], formatter, range: ranges[min(i, ranges.count - 1)], showUnit: false) { onCommit(i, $0) }
            }
        }
    }
}

// MARK: AppKit field

struct NumberTextField: NSViewRepresentable {
    let value: Double?
    let formatter: UnitFormatter
    let range: ClosedRange<Double>?
    let step: Double
    let enabled: Bool
    @Binding var focused: Bool
    @Binding var invalid: String?
    var autoFocus = false
    let onCommit: (Double) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> FocusTextField {
        let f = FocusTextField()
        f.isBordered = false
        f.drawsBackground = false
        f.focusRingType = .none
        f.alignment = .right
        f.font = WDFonts.nsMono(11)
        f.textColor = NSColor(Color.wd.textSecondary)
        f.cell?.lineBreakMode = .byClipping
        f.cell?.isScrollable = true
        f.delegate = context.coordinator
        f.onFocus = { [weak coordinator = context.coordinator] in coordinator?.focusChanged($0) }
        f.setContentHuggingPriority(.defaultLow, for: .horizontal)
        f.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        if autoFocus {
            DispatchQueue.main.async { [weak f] in
                guard let f, let w = f.window else { return }
                w.makeFirstResponder(f)
                f.currentEditor()?.selectAll(nil)
            }
        }
        return f
    }

    func updateNSView(_ f: FocusTextField, context: Context) {
        context.coordinator.parent = self
        f.isEnabled = enabled
        f.textColor = NSColor(enabled ? Color.wd.textSecondary : Color.wd.textDisabled)
        if f.currentEditor() == nil {
            f.stringValue = display
        }
    }

    var display: String { value.map { formatter.string($0) } ?? "—" }

    @MainActor final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: NumberTextField
        init(_ p: NumberTextField) { parent = p }

        func focusChanged(_ on: Bool) {
            parent.focused = on
            if on { parent.invalid = nil }
        }

        func clamp(_ v: Double) -> Double {
            guard let r = parent.range else { return v }
            return min(max(v, r.lowerBound), r.upperBound)
        }

        func commit(_ field: NSTextField) {
            let text = field.stringValue
            if text == parent.display { return }
            guard let v = parent.formatter.parse(text) else {
                parent.invalid = "Could not read “\(text)”"
                field.stringValue = parent.display
                return
            }
            parent.invalid = nil
            let c = clamp(v)
            field.stringValue = parent.formatter.string(c)
            parent.onCommit(c)
        }

        func controlTextDidEndEditing(_ obj: Notification) {
            guard let f = obj.object as? NSTextField else { return }
            commit(f)
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
            guard let f = control as? NSTextField else { return false }
            switch sel {
            case #selector(NSResponder.cancelOperation(_:)):
                f.stringValue = parent.display
                parent.invalid = nil
                f.window?.makeFirstResponder(nil)
                return true
            case #selector(NSResponder.insertNewline(_:)):
                commit(f)
                textView.selectAll(nil)
                return true
            case #selector(NSResponder.moveUp(_:)), #selector(NSResponder.moveDown(_:)):
                let flags = NSEvent.modifierFlags
                let mult = flags.contains(.shift) ? 10 : flags.contains(.option) ? 0.1 : 1
                let base = parent.formatter.parse(f.stringValue) ?? parent.value ?? 0
                let v = clamp(base + (sel == #selector(NSResponder.moveUp(_:)) ? 1 : -1) * parent.step * mult)
                f.stringValue = parent.formatter.string(v)
                textView.selectAll(nil)
                parent.onCommit(v)
                return true
            default:
                return false
            }
        }
    }
}

final class FocusTextField: NSTextField {
    var onFocus: ((Bool) -> Void)?

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok {
            onFocus?(true)
            DispatchQueue.main.async { [weak self] in self?.currentEditor()?.selectAll(nil) }
        }
        return ok
    }

    override func textDidEndEditing(_ notification: Notification) {
        super.textDidEndEditing(notification)
        onFocus?(false)
    }
}

// MARK: Text field

/// Name field: no chrome until hover/focus; commits on Enter/blur, Esc reverts,
/// empty input reverts.
public struct WDTextField: View {
    let value: String
    let placeholder: String
    let chrome: Bool
    let font: Font
    let onCommit: (String) -> Void
    @State private var text = ""
    @State private var hover = false
    @FocusState private var focused: Bool

    public init(_ value: String, placeholder: String = "", chrome: Bool = true, font: Font = .wd.body, onCommit: @escaping (String) -> Void) {
        self.value = value
        self.placeholder = placeholder
        self.chrome = chrome
        self.font = font
        self.onCommit = onCommit
    }

    public var body: some View {
        TextField(placeholder, text: $text)
            .textFieldStyle(.plain)
            .font(font)
            .foregroundStyle(Color.wd.textPrimary)
            .focused($focused)
            .padding(.horizontal, chrome || hover || focused ? 7 : 0)
            .frame(height: .wd.control)
            .background(RoundedRectangle(cornerRadius: .wd.radiusField)
                .fill(focused ? Color.wd.fieldFocused : (chrome || hover) ? Color.wd.controlFill : .clear))
            .overlay(RoundedRectangle(cornerRadius: .wd.radiusField).strokeBorder(focused ? Color.wd.accent : .clear, lineWidth: 1))
            .onHover { hover = $0 }
            .onAppear { text = value }
            .onChange(of: value) { _, v in if !focused { text = v } }
            .onChange(of: focused) { _, f in if !f { commit() } }
            .onSubmit { commit() }
            .onExitCommand { text = value; focused = false }
    }

    private func commit() {
        let t = text.trimmingCharacters(in: .whitespaces)
        if t.isEmpty { text = value; return }
        if t != value { onCommit(t) }
    }
}
