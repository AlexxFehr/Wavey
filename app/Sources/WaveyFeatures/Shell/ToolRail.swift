import SwiftUI
import WaveyDesign

/// Tool rail (36 pt): the active workspace's tools in groups. A button with
/// variants (a small corner mark) offers them on right-click or press and
/// hold; the picked variant stays on the button, CAD style.
struct ToolRail: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        VStack(spacing: .wd.s2) {
            let groups = railItems(ctx.workspace)
            ForEach(Array(groups.enumerated()), id: \.offset) { i, group in
                if i > 0 {
                    Rectangle().fill(Color.wd.hairline).frame(width: .wd.railSeparator, height: 1).padding(.vertical, 5)
                }
                ForEach(group, id: \.self) { item in
                    RailButton(ctx: ctx, item: item)
                }
            }
            Spacer()
        }
        .padding(.top, .wd.railTopPadding)
        .frame(width: .wd.railWidth)
        .background(Color.wd.surfaceChrome)
    }
}

private struct RailButton: View {
    @Bindable var ctx: WindowContext
    let item: RailItem
    @State private var flyout = false

    var body: some View {
        let t = ctx.railTool(item)
        let key = t.key(in: ctx.workspace).map { " (\($0))" } ?? ""
        let more = item.variants.count > 1 ? " · right-click for more" : ""
        WDIconButton(t.icon, isOn: item.variants.contains(ctx.activeTool), help: t.title + key + more) { ctx.useTool(t) }
            .overlay(alignment: .bottomTrailing) {
                if item.variants.count > 1 {
                    Path { p in
                        p.move(to: CGPoint(x: 5, y: 0)); p.addLine(to: CGPoint(x: 5, y: 5)); p.addLine(to: CGPoint(x: 0, y: 5)); p.closeSubpath()
                    }
                    .fill(Color.wd.textTertiary)
                    .frame(width: 5, height: 5)
                    .padding(3)
                    .allowsHitTesting(false)
                }
            }
            .simultaneousGesture(LongPressGesture(minimumDuration: 0.4).onEnded { _ in if item.variants.count > 1 { flyout = true } })
            .contextMenu {
                if item.variants.count > 1 {
                    ForEach(item.variants, id: \.self) { v in
                        Button(v.title) { ctx.useTool(v, from: item) }
                    }
                }
            }
            .popover(isPresented: $flyout, arrowEdge: .trailing) {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(item.variants, id: \.self) { v in
                        Button {
                            flyout = false
                            ctx.useTool(v, from: item)
                        } label: {
                            HStack(spacing: .wd.s8) {
                                WDIcon(v.icon).foregroundStyle(v == t ? Color.wd.accentText : Color.wd.textSecondary)
                                Text(v.title).font(.wd.body).foregroundStyle(Color.wd.textPrimary)
                                Spacer()
                                if let k = v.key(in: ctx.workspace) { Text(k).font(.wd.chip).foregroundStyle(Color.wd.textTertiary) }
                            }
                            .padding(.horizontal, .wd.s10)
                            .frame(width: 230, height: .wd.row + 4)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.vertical, .wd.s4)
                .background(Color.wd.surfacePanel)
            }
    }
}

extension WindowContext {
    /// The tool a rail button shows: the last variant picked, else its first.
    func railTool(_ item: RailItem) -> ToolID {
        railChoice[item.key].flatMap { item.variants.contains($0) ? $0 : nil } ?? item.variants[0]
    }

    /// Activates a tool (remembering the variant on its rail button). The
    /// stack and line-array tools open their editors instead.
    func useTool(_ t: ToolID, from item: RailItem? = nil) {
        let item = item ?? railItems(workspace).flatMap { $0 }.first { $0.variants.contains(t) }
        if let item { railChoice[item.key] = t }
        switch t {
        case .stackTool: newStack()
        case .lineArrayTool: newLineArray()
        case .receiverGrid: tool.activate(.receiverGrid)
        default: tool.activate(t)
        }
    }
}
