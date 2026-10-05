import AppKit
import SwiftUI

/// How a table cell edits its value. Edits apply to every selected row when
/// the edited row is part of the selection.
public enum WDCellEditor<Row> {
    case none
    case text((_ rows: [Row], _ value: String) -> Void)
    case number(UnitFormatter, (_ rows: [Row], _ value: Double) -> Void)
    case checkbox((Row) -> Bool, (_ rows: [Row], _ value: Bool) -> Void)
    case menu((Row) -> [(String, String)], (_ rows: [Row], _ id: String) -> Void)
}

public struct WDTableColumn<Row> {
    public let id: String
    public let title: String
    public let width: CGFloat
    public let numeric: Bool
    public let value: (Row) -> String
    public let color: ((Row) -> Color?)?
    public let editor: WDCellEditor<Row>

    /// Numeric columns are mono and right-aligned.
    public init(_ id: String, _ title: String, width: CGFloat, numeric: Bool = false, color: ((Row) -> Color?)? = nil,
                editor: WDCellEditor<Row> = .none, value: @escaping (Row) -> String) {
        self.id = id
        self.title = title
        self.width = width
        self.numeric = numeric
        self.value = value
        self.color = color
        self.editor = editor
    }
}

/// Dense data table (NSTableView): 24 pt rows, hairline header, row
/// dividers, global selection, inline editing, ⌘C copies TSV.
public struct WDTable<Row: Identifiable>: NSViewRepresentable where Row.ID: Hashable {
    let rows: [Row]
    let columns: [WDTableColumn<Row>]
    let selection: Set<Row.ID>
    let onSelect: (_ ids: [Row.ID], _ primary: Row.ID?) -> Void
    let emptyText: String

    public init(_ rows: [Row], columns: [WDTableColumn<Row>], selection: Set<Row.ID>, emptyText: String = "Nothing here",
                onSelect: @escaping ([Row.ID], Row.ID?) -> Void) {
        self.rows = rows
        self.columns = columns
        self.selection = selection
        self.onSelect = onSelect
        self.emptyText = emptyText
    }

    public func makeCoordinator() -> Coordinator { Coordinator(self) }

    public func makeNSView(context: Context) -> NSScrollView {
        let table = CopyTableView()
        table.coordinatorCopy = { [weak coordinator = context.coordinator] in coordinator?.tsv() ?? "" }
        table.style = .plain
        table.backgroundColor = .clear
        table.rowHeight = .wd.row
        table.intercellSpacing = NSSize(width: 0, height: 0)
        table.gridStyleMask = []
        table.usesAlternatingRowBackgroundColors = false
        table.allowsMultipleSelection = true
        table.allowsColumnReordering = false
        table.allowsColumnResizing = true
        table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        table.headerView = HeaderView()
        table.focusRingType = .none
        table.selectionHighlightStyle = .regular
        table.delegate = context.coordinator
        table.dataSource = context.coordinator
        let scroll = NSScrollView()
        table.autoresizingMask = [.width, .height]
        scroll.documentView = table
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.borderType = .noBorder
        context.coordinator.table = table
        context.coordinator.syncColumns()
        return scroll
    }

    public func updateNSView(_ scroll: NSScrollView, context: Context) {
        let c = context.coordinator
        let oldIDs = c.parent.rows.map(\.id)
        c.parent = self
        c.syncColumns()
        if oldIDs != rows.map(\.id) || !c.loaded {
            c.loaded = true
            c.table?.reloadData()
        } else {
            c.table?.reloadData(forRowIndexes: IndexSet(integersIn: 0..<rows.count), columnIndexes: IndexSet(integersIn: 0..<columns.count))
        }
        c.applySelection()
        // Keep the table as wide as the visible area so rows are laid out.
        if let t = c.table, t.frame.width < scroll.contentSize.width {
            t.frame.size.width = scroll.contentSize.width
        }
    }

    @MainActor public final class Coordinator: NSObject, NSTableViewDelegate, NSTableViewDataSource, NSTextFieldDelegate {
        var parent: WDTable
        weak var table: NSTableView?
        var loaded = false
        private var syncing = false

        init(_ p: WDTable) { parent = p }

        func syncColumns() {
            guard let table else { return }
            let ids = parent.columns.map(\.id)
            if table.tableColumns.map(\.identifier.rawValue) == ids { return }
            table.tableColumns.forEach(table.removeTableColumn)
            for (i, col) in parent.columns.enumerated() {
                let c = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(col.id))
                c.width = col.width
                c.minWidth = 40
                let h = HeaderCell(textCell: col.title)
                h.alignment = col.numeric ? .right : .left
                c.headerCell = h
                c.isEditable = false
                if i == 0 { c.resizingMask = .userResizingMask }
                table.addTableColumn(c)
            }
        }

        func applySelection() {
            guard let table else { return }
            let want = IndexSet(parent.rows.enumerated().filter { parent.selection.contains($0.element.id) }.map(\.offset))
            if table.selectedRowIndexes != want {
                syncing = true
                table.selectRowIndexes(want, byExtendingSelection: false)
                if let first = want.first { table.scrollRowToVisible(first) }
                syncing = false
            }
        }

        public func numberOfRows(in tableView: NSTableView) -> Int { parent.rows.count }

        public func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? { RowView() }

        public func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
            guard let tableColumn, let ci = parent.columns.firstIndex(where: { $0.id == tableColumn.identifier.rawValue }),
                  row < parent.rows.count else { return nil }
            let col = parent.columns[ci]
            let item = parent.rows[row]
            let selected = parent.selection.contains(item.id)
            switch col.editor {
            case .checkbox(let get, _):
                let b = CheckCell(on: get(item))
                b.onToggle = { [weak self] v in self?.edit(row: row, column: ci) { rows in
                    if case .checkbox(_, let set) = col.editor { set(rows, v) }
                } }
                return b
            default:
                let f = NSTextField(labelWithString: col.value(item))
                f.font = col.numeric ? WDFonts.nsMono(11) : WDFonts.nsUI(11.5)
                f.alignment = col.numeric ? .right : .left
                f.lineBreakMode = .byTruncatingTail
                var color = NSColor(Color.wd.textSecondary)
                if ci == 0, selected { color = NSColor(Color.wd.accentTextOnFill) }
                if let c = col.color?(item) { color = NSColor(c) }
                f.textColor = color
                f.tag = ci
                switch col.editor {
                case .text, .number:
                    f.isEditable = true
                    f.isSelectable = true
                    f.delegate = self
                    f.focusRingType = .none
                default: break
                }
                let cell = NSTableCellView()
                cell.addSubview(f)
                cell.textField = f
                f.translatesAutoresizingMaskIntoConstraints = false
                NSLayoutConstraint.activate([
                    f.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 8),
                    f.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -8),
                    f.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
                ])
                if case .menu(let options, let pick) = col.editor {
                    let click = NSClickGestureRecognizer(target: self, action: #selector(menuClick(_:)))
                    cell.addGestureRecognizer(click)
                    menus[ObjectIdentifier(cell)] = (row, ci, options(item), pick)
                }
                return cell
            }
        }

        private var menus: [ObjectIdentifier: (Int, Int, [(String, String)], ([Row], String) -> Void)] = [:]

        @objc func menuClick(_ g: NSClickGestureRecognizer) {
            guard let view = g.view, let (row, ci, options, pick) = menus[ObjectIdentifier(view)] else { return }
            let menu = NSMenu()
            for (id, title) in options {
                let item = NSMenuItem(title: title, action: #selector(menuPick(_:)), keyEquivalent: "")
                item.target = self
                item.representedObject = MenuChoice(row: row, column: ci, id: id, pick: pick)
                menu.addItem(item)
            }
            menu.popUp(positioning: nil, at: NSPoint(x: 0, y: view.bounds.height), in: view)
        }

        final class MenuChoice: NSObject {
            let row: Int, column: Int, id: String
            let pick: ([Row], String) -> Void
            init(row: Int, column: Int, id: String, pick: @escaping ([Row], String) -> Void) {
                self.row = row; self.column = column; self.id = id; self.pick = pick
            }
        }

        @objc func menuPick(_ item: NSMenuItem) {
            guard let c = item.representedObject as? MenuChoice else { return }
            edit(row: c.row, column: c.column) { c.pick($0, c.id) }
        }

        /// Rows an edit applies to: the selection if the edited row is in it.
        private func edit(row: Int, column: Int, _ apply: ([Row]) -> Void) {
            guard row < parent.rows.count else { return }
            let item = parent.rows[row]
            if parent.selection.contains(item.id) {
                apply(parent.rows.filter { parent.selection.contains($0.id) })
            } else {
                apply([item])
            }
        }

        public func controlTextDidEndEditing(_ obj: Notification) {
            guard let f = obj.object as? NSTextField, let table else { return }
            let row = table.row(for: f)
            let ci = f.tag
            guard row >= 0, ci < parent.columns.count else { return }
            let col = parent.columns[ci]
            let text = f.stringValue
            switch col.editor {
            case .text(let set):
                let t = text.trimmingCharacters(in: .whitespaces)
                if t.isEmpty || t == col.value(parent.rows[row]) { f.stringValue = col.value(parent.rows[row]); return }
                edit(row: row, column: ci) { set($0, t) }
            case .number(let fmt, let set):
                if text == col.value(parent.rows[row]) { return }
                guard let v = fmt.parse(text) else { f.stringValue = col.value(parent.rows[row]); NSSound.beep(); return }
                edit(row: row, column: ci) { set($0, v) }
            default: break
            }
        }

        public func control(_ control: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
            if sel == #selector(NSResponder.cancelOperation(_:)), let f = control as? NSTextField, let table {
                let row = table.row(for: f)
                if row >= 0, f.tag < parent.columns.count { f.stringValue = parent.columns[f.tag].value(parent.rows[row]) }
                f.window?.makeFirstResponder(table)
                return true
            }
            return false
        }

        public func tableViewSelectionDidChange(_ notification: Notification) {
            guard !syncing, let table else { return }
            let ids = table.selectedRowIndexes.compactMap { $0 < parent.rows.count ? parent.rows[$0].id : nil }
            let primary = table.clickedRow >= 0 && table.clickedRow < parent.rows.count ? parent.rows[table.clickedRow].id : ids.last
            if Set(ids) != parent.selection { parent.onSelect(ids, primary) }
        }

        func tsv() -> String {
            guard let table else { return "" }
            let header = parent.columns.map(\.title).joined(separator: "\t")
            let lines = table.selectedRowIndexes.map { r in parent.columns.map { $0.value(parent.rows[r]) }.joined(separator: "\t") }
            return ([header] + lines).joined(separator: "\n")
        }
    }
}

// MARK: AppKit pieces

final class CopyTableView: NSTableView {
    var coordinatorCopy: (() -> String)?
    @objc func copy(_ sender: Any?) {
        guard let s = coordinatorCopy?(), !selectedRowIndexes.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(s, forType: .string)
    }
    override func drawBackground(inClipRect clipRect: NSRect) {}
}

final class RowView: NSTableRowView {
    override func drawBackground(in dirtyRect: NSRect) {
        NSColor(Color.wd.rowDivider).setFill()
        NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill()
    }
    override func drawSelection(in dirtyRect: NSRect) {
        NSColor(Color.wd.selectionRow).setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: bounds.height - 1).fill()
    }
    override var isEmphasized: Bool { get { false } set {} }
}

final class HeaderView: NSTableHeaderView {
    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        NSColor(Color.wd.hairline).setFill()
        NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill()
    }
}

final class HeaderCell: NSTableHeaderCell {
    override func draw(withFrame cellFrame: NSRect, in controlView: NSView) {
        let r = cellFrame.insetBy(dx: 8, dy: 0)
        let p = NSMutableParagraphStyle()
        p.alignment = alignment
        p.lineBreakMode = .byTruncatingTail
        let attrs: [NSAttributedString.Key: Any] = [
            .font: WDFonts.nsUI(10.5, .medium),
            .foregroundColor: NSColor(Color.wd.textTertiary),
            .paragraphStyle: p,
        ]
        let h = stringValue.size(withAttributes: attrs).height
        stringValue.draw(in: NSRect(x: r.minX, y: r.midY - h / 2, width: r.width, height: h), withAttributes: attrs)
    }
    override func cellSize(forBounds rect: NSRect) -> NSSize { NSSize(width: rect.width, height: .wd.row) }
}

final class CheckCell: NSView {
    var on: Bool
    var onToggle: ((Bool) -> Void)?
    init(on: Bool) {
        self.on = on
        super.init(frame: .zero)
    }
    required init?(coder: NSCoder) { fatalError() }
    override func draw(_ dirtyRect: NSRect) {
        // Left-aligned under the header title, so it stays put when the
        // (last, stretching) column is resized.
        let r = NSRect(x: 8, y: (bounds.height - 14) / 2, width: 14, height: 14)
        let path = NSBezierPath(roundedRect: r, xRadius: 3, yRadius: 3)
        NSColor(on ? Color.wd.accent : Color.wd.controlFill).setFill()
        path.fill()
        if on {
            let c = NSBezierPath()
            c.move(to: NSPoint(x: r.minX + 3.5, y: r.midY))
            c.line(to: NSPoint(x: r.minX + 6, y: r.minY + 3.5))
            c.line(to: NSPoint(x: r.maxX - 3, y: r.maxY - 3.5))
            c.lineWidth = 1.6
            c.lineCapStyle = .round
            NSColor(Color.wd.surfaceChrome).setStroke()
            c.stroke()
        }
    }
    override var isFlipped: Bool { false }
    override func mouseDown(with event: NSEvent) {
        on.toggle()
        needsDisplay = true
        onToggle?(on)
    }
}

// MARK: - Static rendering (offscreen snapshots)

private struct StaticTablesKey: EnvironmentKey {
    static let defaultValue = false
}

public extension EnvironmentValues {
    /// Draw tables as plain SwiftUI (offscreen snapshots cannot capture the
    /// AppKit table, and it hides the content drawn before it).
    var wdStaticTables: Bool {
        get { self[StaticTablesKey.self] }
        set { self[StaticTablesKey.self] = newValue }
    }
}

/// The look of `WDTable` without AppKit: header and rows, no editing.
public struct WDStaticTable<Row: Identifiable>: View where Row.ID: Hashable {
    let rows: [Row]
    let columns: [WDTableColumn<Row>]
    let selection: Set<Row.ID>

    public init(_ rows: [Row], columns: [WDTableColumn<Row>], selection: Set<Row.ID>) {
        self.rows = rows
        self.columns = columns
        self.selection = selection
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                ForEach(columns.indices, id: \.self) { i in
                    Text(columns[i].title).font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
                        .frame(width: columns[i].width, alignment: columns[i].numeric ? .trailing : .leading).padding(.horizontal, 6)
                }
                Spacer(minLength: 0)
            }
            .frame(height: .wd.row)
            .background(Color.wd.surfaceBar)
            ForEach(rows) { r in
                HStack(spacing: 0) {
                    ForEach(columns.indices, id: \.self) { i in
                        Text(columns[i].value(r)).font(columns[i].numeric ? .wd.numeric : .wd.body)
                            .foregroundStyle(columns[i].color?(r) ?? Color.wd.textSecondary).lineLimit(1)
                            .frame(width: columns[i].width, alignment: columns[i].numeric ? .trailing : .leading).padding(.horizontal, 6)
                    }
                    Spacer(minLength: 0)
                }
                .frame(height: .wd.row)
                .background(selection.contains(r.id) ? Color.wd.selectionRow : .clear)
                .overlay(alignment: .bottom) { Rectangle().fill(Color.wd.rowDivider).frame(height: 1) }
            }
            Spacer(minLength: 0)
        }
    }
}
