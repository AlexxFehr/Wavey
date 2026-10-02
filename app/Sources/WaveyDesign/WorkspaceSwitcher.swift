import SwiftUI

/// Peer segments in a track (the top-bar workspace switcher). Behaves as a
/// tab bar; arrow keys move between segments.
public struct WDWorkspaceSwitcher<T: Hashable>: View {
    let items: [(T, String, WDIconName)]
    let selection: T
    let onSelect: (T) -> Void

    public init(_ items: [(T, String, WDIconName)], selection: T, onSelect: @escaping (T) -> Void) {
        self.items = items
        self.selection = selection
        self.onSelect = onSelect
    }

    public var body: some View {
        HStack(spacing: 0) {
            ForEach(items, id: \.0) { item in
                Segment(title: item.1, icon: item.2, active: item.0 == selection) { onSelect(item.0) }
            }
        }
        .padding(.wd.s2)
        .background(RoundedRectangle(cornerRadius: .wd.radiusTrack).fill(Color.wd.switcherTrack))
        .focusable()
        .focusEffectDisabled()
        .onKeyPress(.leftArrow) { move(-1); return .handled }
        .onKeyPress(.rightArrow) { move(1); return .handled }
        .accessibilityElement(children: .contain)
        .accessibilityAddTraits(.isTabBar)
    }

    private func move(_ d: Int) {
        guard let i = items.firstIndex(where: { $0.0 == selection }) else { return }
        onSelect(items[(i + d + items.count) % items.count].0)
    }

    struct Segment: View {
        let title: String
        let icon: WDIconName
        let active: Bool
        let action: () -> Void
        @State private var hover = false

        var body: some View {
            Button(action: action) {
                HStack(spacing: 7) {
                    WDIcon(icon).foregroundStyle(active ? Color.wd.accentText : hover ? Color.wd.textSecondary : Color.wd.textTertiary)
                    Text(title).font(.wd.switcher).foregroundStyle(active ? Color.wd.textPrimary : hover ? Color.wd.textSecondary : Color.wd.textTertiary)
                }
                .padding(.horizontal, 14)
                .frame(height: .wd.segment)
                .background(RoundedRectangle(cornerRadius: .wd.radiusSegment).fill(active ? Color.wd.switcherActive : .clear))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onHover { hover = $0 }
            .accessibilityLabel(title)
            .accessibilityAddTraits(active ? [.isSelected] : [])
        }
    }
}
