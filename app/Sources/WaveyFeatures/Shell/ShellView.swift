import AppKit
import SwiftUI
import WaveyDesign
import WaveyModel
import WaveyViewport

/// The window: top bar, tool rail, outliner, viewport region with dock,
/// inspector and status bar. Its structure never changes between
/// workspaces; only region contents do.
public struct ShellView: View {
    @Bindable var ctx: WindowContext

    public init(context: WindowContext) {
        ctx = context
    }

    public var body: some View {
        VStack(spacing: 0) {
            TopBar(ctx: ctx)
            HStack(spacing: 0) {
                ToolRail(ctx: ctx)
                if ctx.leftVisible {
                    LeftPanel(ctx: ctx)
                        .frame(width: CGFloat(ctx.state.leftWidth))
                        .background(Color.wd.surfacePanel)
                    PanelDivider(axis: .vertical) { d in
                        ctx.state.leftWidth = min(max(ctx.state.leftWidth + d, 200), 360)
                    }
                }
                ViewportRegion(ctx: ctx)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                if ctx.inspectorVisible {
                    PanelDivider(axis: .vertical) { d in
                        ctx.state.inspectorWidth = min(max(ctx.state.inspectorWidth - d, 260), 420)
                    }
                    InspectorHost(ctx: ctx)
                        .frame(width: CGFloat(ctx.state.inspectorWidth))
                        .background(Color.wd.surfacePanel)
                }
            }
            StatusBar(ctx: ctx)
        }
        .background(Color.wd.surfaceChrome)
        .environment(\.colorScheme, .dark)
        .overlay {
            if ctx.palettePresented {
                CommandPalette(ctx: ctx)
            }
        }
        .sheet(isPresented: $ctx.materialLibraryPresented) {
            MaterialLibrarySheet(ctx: ctx)
        }
    }
}

/// 1 pt visual divider with a 6 pt drag area. Double-click calls `onDouble`.
struct PanelDivider: View {
    enum Axis { case vertical, horizontal }
    let axis: Axis
    var onDouble: (() -> Void)?
    let onDrag: (Double) -> Void
    @State private var last: CGFloat = 0

    init(axis: Axis, onDouble: (() -> Void)? = nil, onDrag: @escaping (Double) -> Void) {
        self.axis = axis
        self.onDouble = onDouble
        self.onDrag = onDrag
    }

    var body: some View {
        ZStack {
            Rectangle().fill(Color.wd.surfaceChrome)
                .frame(width: axis == .vertical ? 1 : nil, height: axis == .horizontal ? 1 : nil)
        }
        .frame(width: axis == .vertical ? 1 : nil, height: axis == .horizontal ? 1 : nil)
        .overlay {
            Color.clear
                .frame(width: axis == .vertical ? 6 : nil, height: axis == .horizontal ? 6 : nil)
                .contentShape(Rectangle())
                .onHover { inside in
                    if inside { (axis == .vertical ? NSCursor.resizeLeftRight : NSCursor.resizeUpDown).push() } else { NSCursor.pop() }
                }
                .gesture(DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { g in
                        let v = axis == .vertical ? g.translation.width : g.translation.height
                        onDrag(Double(v - last))
                        last = v
                    }
                    .onEnded { _ in last = 0 })
                .onTapGesture(count: 2) { onDouble?() }
        }
        .zIndex(10)
    }
}

/// Region header (30 pt): chips scroll horizontally, never wrap.
struct RegionHeader<Leading: View, Trailing: View>: View {
    let leading: Leading
    let trailing: Trailing
    init(@ViewBuilder leading: () -> Leading, @ViewBuilder trailing: () -> Trailing) {
        self.leading = leading()
        self.trailing = trailing()
    }
    var body: some View {
        HStack(spacing: .wd.s8) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: .wd.s4) { leading }.padding(.horizontal, .wd.s8)
            }
            trailing.padding(.trailing, .wd.s8)
        }
        .frame(height: .wd.viewportHeader)
        .background(Color.wd.surfaceBar)
    }
}

/// Small label inside header chip rows ("Snap", "Show", "Quantity").
struct HeaderLabel: View {
    let text: String
    init(_ t: String) { text = t }
    var body: some View {
        Text(text).font(.wd.body).foregroundStyle(Color.wd.textTertiary).padding(.horizontal, .wd.s4).fixedSize()
    }
}
