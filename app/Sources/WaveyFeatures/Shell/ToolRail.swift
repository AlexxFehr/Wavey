import SwiftUI
import WaveyDesign

/// Tool rail (36 pt): the active workspace's tools in groups. Exactly one is active.
struct ToolRail: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        VStack(spacing: .wd.s2) {
            let groups = railTools(ctx.workspace)
            ForEach(Array(groups.enumerated()), id: \.offset) { i, group in
                if i > 0 {
                    Rectangle().fill(Color.wd.hairline).frame(width: .wd.railSeparator, height: 1).padding(.vertical, 5)
                }
                ForEach(group, id: \.self) { t in
                    let key = t.key(in: ctx.workspace).map { " (\($0))" } ?? ""
                    WDIconButton(t.icon, isOn: ctx.activeTool == t, help: t.title + key) { ctx.tool.activate(t) }
                }
            }
            Spacer()
        }
        .padding(.top, .wd.railTopPadding)
        .frame(width: .wd.railWidth)
        .background(Color.wd.surfaceChrome)
    }
}
