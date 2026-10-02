import SwiftUI

public struct WDDropdownItem<ID: Hashable>: Identifiable {
    public let id: ID
    public let title: String
    public let swatch: Color?
    public let detail: String?
    public let icon: WDIconName?

    public init(_ id: ID, _ title: String, swatch: Color? = nil, detail: String? = nil, icon: WDIconName? = nil) {
        self.id = id
        self.title = title
        self.swatch = swatch
        self.detail = detail
        self.icon = icon
    }
}

/// 22 pt dropdown: label + chevron, opens a list popover (with search when
/// there are more than 12 items).
public struct WDDropdown<ID: Hashable>: View {
    let items: [WDDropdownItem<ID>]
    let selection: ID?
    let placeholder: String
    let onSelect: (ID) -> Void
    @State private var open = false
    @State private var hover = false
    @Environment(\.isEnabled) private var enabled

    /// `selection` nil shows "—" (mixed) or the placeholder.
    public init(_ items: [WDDropdownItem<ID>], selection: ID?, placeholder: String = "—", onSelect: @escaping (ID) -> Void) {
        self.items = items
        self.selection = selection
        self.placeholder = placeholder
        self.onSelect = onSelect
    }

    public var body: some View {
        let current = items.first { $0.id == selection }
        Button { open.toggle() } label: {
            HStack(spacing: .wd.s6) {
                if let s = current?.swatch { Circle().fill(s).frame(width: 8, height: 8) }
                Text(current?.title ?? placeholder)
                    .font(.wd.body)
                    .foregroundStyle(current == nil ? Color.wd.textTertiary : Color.wd.textSecondary)
                    .lineLimit(1)
                Spacer(minLength: .wd.s4)
                WDIcon(.chevronDown, size: 12).foregroundStyle(Color.wd.textTertiary)
            }
            .padding(.horizontal, 7)
            .frame(height: .wd.control)
            .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(hover && enabled ? Color.wd.controlHover : Color.wd.controlFill))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
        .popover(isPresented: $open, arrowEdge: .bottom) {
            WDDropdownList(items: items, selection: selection) { id in
                open = false
                onSelect(id)
            }
        }
        .accessibilityValue(current?.title ?? placeholder)
    }
}

struct WDDropdownList<ID: Hashable>: View {
    let items: [WDDropdownItem<ID>]
    let selection: ID?
    let onSelect: (ID) -> Void
    @State private var query = ""
    @State private var hover: ID?

    var filtered: [WDDropdownItem<ID>] {
        query.isEmpty ? items : items.filter { $0.title.localizedCaseInsensitiveContains(query) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: .wd.s4) {
            if items.count > 12 {
                TextField("Search", text: $query)
                    .textFieldStyle(.plain)
                    .font(.wd.body)
                    .padding(.horizontal, 7)
                    .frame(height: .wd.control)
                    .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.controlFill))
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(filtered) { item in
                        HStack(spacing: .wd.s6) {
                            if let s = item.swatch { Circle().fill(s).frame(width: 8, height: 8) }
                            if let i = item.icon { WDIcon(i, size: 12).foregroundStyle(Color.wd.textTertiary) }
                            Text(item.title).font(.wd.body).lineLimit(1)
                                .foregroundStyle(item.id == selection ? Color.wd.accentTextOnFill : Color.wd.textSecondary)
                            Spacer(minLength: .wd.s8)
                            if let d = item.detail { Text(d).font(.wd.meta).foregroundStyle(Color.wd.textTertiary) }
                        }
                        .padding(.horizontal, .wd.s8)
                        .frame(height: .wd.row)
                        .background(RoundedRectangle(cornerRadius: .wd.radiusField)
                            .fill(item.id == selection ? Color.wd.selectionRow : hover == item.id ? Color.wd.rowHover : .clear))
                        .contentShape(Rectangle())
                        .onHover { if $0 { hover = item.id } }
                        .onTapGesture { onSelect(item.id) }
                    }
                }
            }
            .frame(maxHeight: 320)
        }
        .padding(.wd.s6)
        .frame(width: 260)
        .background(Color.wd.surfacePanel)
    }
}
