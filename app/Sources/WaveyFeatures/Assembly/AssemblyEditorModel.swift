import Foundation
import Observation
import WaveyCore
import WaveyModel

/// What an assembly editor window edits.
public enum AssemblyKindTag: String, Sendable { case stack, lineArray }

/// Draft state of one stack or line-array editor window. Edits stay local
/// until Insert (new group) or Apply (existing group).
@MainActor @Observable
public final class AssemblyEditorModel {
    unowned let ctx: WindowContext
    /// The group being edited; nil for a new assembly.
    public private(set) var groupID: UInt64?
    public let kind: AssemblyKindTag
    public var name: String
    public private(set) var assembly: AssemblyRec
    public private(set) var layout: AssemblyLayoutRec?
    public private(set) var error: String?
    /// Element rows selected in the table / preview.
    public var selectedRows: Set<Int> = []
    public var view = "side"

    // Prediction (free field), shown when enabled.
    public var predict = true { didSet { schedulePrediction() } }
    public var predictFrequencies: [Double] = [] { didSet { schedulePrediction() } }
    public var polarVertical = false { didSet { schedulePrediction() } }
    public private(set) var polar: PredictionRec?
    public private(set) var distance: PredictionRec?
    /// Stacks: the same line behind the stack (rear), for front/rear comparison.
    public private(set) var distanceRear: PredictionRec?
    /// Stacks: band the rear-rejection tuning optimises.
    public var tuneLo = 40.0
    public var tuneHi = 100.0
    public private(set) var tuneResult: Double?
    /// Line arrays: preview zoomed on the array or showing the coverage.
    public var showCoverage = false
    public private(set) var predicting = false
    @ObservationIgnored private var predictionTask: Task<Void, Never>?

    // Line-array audience (also used by the preview's aim rays).
    public var audienceFirst = 5.0
    public var audienceLast = 30.0
    public var listenerZ = 1.2
    public var rakeDeg = 0.0

    init(ctx: WindowContext, kind: AssemblyKindTag, group: UInt64? = nil) {
        self.ctx = ctx
        self.kind = kind
        groupID = group
        let store = ctx.store
        if let g = group, let a = store.doc.groupAssembly(group: g) {
            assembly = a
            name = store.groupsById[g]?.name ?? "Array"
        } else {
            let room = store.room
            let centre = room.map { Point3(x: ($0.boundsMin.x + $0.boundsMax.x) / 2, y: ($0.boundsMin.y + $0.boundsMax.y) / 2, z: 0) } ?? Point3(x: 0, y: 0, z: 0)
            switch kind {
            case .stack:
                // A sub from the project, else from the stock library (added to the project).
                if !store.speakerModels.contains(where: { ($0.nominalMaxHz ?? $0.maxHz) <= 250 }),
                   let e = SpeakerLibrary.shared.entries.first(where: { ($0.nominalMaxHz ?? $0.maxHz) <= 250 && $0.hasBalloon }) {
                    ctx.useLibraryFile(e.path)
                }
                let sub = store.speakerModels.first { ($0.nominalMaxHz ?? $0.maxHz) <= 250 } ?? store.speakerModels.first
                let preset = StackPresetRec.cardioid(modelId: sub?.id ?? "", forward: 2, reversed: 1)
                let elements = (try? store.doc.stackPresetElements(preset: preset)) ?? []
                assembly = AssemblyRec(origin: centre, yawDeg: 0, kind: .stack(preset: preset, elements: elements))
                name = "Sub stack \(store.groups.count + 1)"
            case .lineArray:
                // A rigged module from the project, else from the stock library.
                if !store.speakerModels.contains(where: { store.doc.speakerRigging(modelId: $0.id) != nil }),
                   let e = SpeakerLibrary.shared.entries.first(where: \.hasRigging) {
                    ctx.useLibraryFile(e.path)
                }
                let module = store.speakerModels.first { store.doc.speakerRigging(modelId: $0.id) != nil }
                let rig = module.flatMap { store.doc.speakerRigging(modelId: $0.id) }
                let frame = rig?.frames.first { $0.flown }
                let n = 8
                let trim = max(3, (room.map { $0.boundsMax.z } ?? 8) - 1)
                var origin = centre
                origin.z = trim
                let recipe = LineArrayRec(modelId: module?.id ?? "", frame: frame?.key, frameSplayDeg: 0,
                                          splaysDeg: Array(repeating: 2, count: n - 1), tilt: .manual(deg: 0),
                                          elements: Array(repeating: ElementSettingsRec(gainDb: 0, delayMs: 0, polarityInverted: false, muted: false, filters: []), count: n),
                                          auto: nil)
                assembly = AssemblyRec(origin: origin, yawDeg: 0, kind: .lineArray(recipe: recipe))
                name = "Line array \(store.groups.count + 1)"
            }
        }
        if case .lineArray(let r) = assembly.kind, let a = r.auto {
            audienceFirst = a.firstM; audienceLast = a.lastM; listenerZ = a.listenerZ; rakeDeg = a.rakeDeg
        }
        predictFrequencies = kind == .stack ? [31.5, 50, 80] : [250, 1000, 4000]
        polarVertical = kind == .lineArray
        refresh()
    }

    public var store: DocumentStore { ctx.store }

    public var stackRecipe: (StackPresetRec, [StackElementRec])? {
        if case .stack(let p, let e) = assembly.kind { return (p, e) }
        return nil
    }

    public var lineRecipe: LineArrayRec? {
        if case .lineArray(let r) = assembly.kind { return r }
        return nil
    }

    public var rigging: RiggingRec? { lineRecipe.flatMap { store.doc.speakerRigging(modelId: $0.modelId) } }

    public var elementCount: Int {
        switch assembly.kind {
        case .stack(_, let e): e.count
        case .lineArray(let r): r.elements.count
        }
    }

    // MARK: Edits

    public func setAssembly(_ a: AssemblyRec) {
        assembly = a
        refresh()
    }

    public func setOrigin(_ p: Point3) { var a = assembly; a.origin = p; setAssembly(a) }
    public func setYaw(_ deg: Double) { var a = assembly; a.yawDeg = deg; setAssembly(a) }

    public func editStack(_ f: (inout StackPresetRec, inout [StackElementRec]) -> Void) {
        guard case .stack(var p, var e) = assembly.kind else { return }
        f(&p, &e)
        var a = assembly
        a.kind = .stack(preset: p, elements: e)
        setAssembly(a)
    }

    /// Regenerates the stack from a preset (elements are replaced).
    public func applyPreset(_ p: StackPresetRec) {
        do {
            let e = try store.doc.stackPresetElements(preset: p)
            var a = assembly
            a.kind = .stack(preset: p, elements: e)
            setAssembly(a)
        } catch { self.error = "\(error)" }
    }

    public func editLine(_ f: (inout LineArrayRec) -> Void) {
        guard case .lineArray(var r) = assembly.kind else { return }
        f(&r)
        var a = assembly
        a.kind = .lineArray(recipe: r)
        setAssembly(a)
    }

    public func setBoxCount(_ n: Int) {
        let limit = currentFrame?.maxBoxes.map(Int.init) ?? 64
        let n = min(max(1, n), limit)
        editLine { r in
            let last = r.splaysDeg.last ?? 0
            if r.splaysDeg.count > n - 1 { r.splaysDeg.removeLast(r.splaysDeg.count - (n - 1)) }
            while r.splaysDeg.count < n - 1 { r.splaysDeg.append(last) }
            let blank = ElementSettingsRec(gainDb: 0, delayMs: 0, polarityInverted: false, muted: false, filters: [])
            if r.elements.count > n { r.elements.removeLast(r.elements.count - n) }
            while r.elements.count < n { r.elements.append(r.elements.last ?? blank) }
        }
    }

    /// Switches between flown and ground-stacked: the frame, the tilt (a
    /// ground stack stands level, angled by its base) and the origin height
    /// (a ground stack stands on the floor), and the box count within the frame's limit.
    public func setMount(frame f: RigFrameRec) {
        var a = assembly
        if case .lineArray(var r) = a.kind {
            r.frame = f.key
            r.frameSplayDeg = 0
            r.tilt = .manual(deg: 0)
            a.kind = .lineArray(recipe: r)
        }
        if !f.flown { a.origin.z = 0 } else if a.origin.z < 1 { a.origin.z = max(3, (store.room?.boundsMax.z ?? 8) - 1) }
        setAssembly(a)
        if let mx = f.maxBoxes, elementCount > Int(mx) { setBoxCount(Int(mx)) }
    }

    public var currentFrame: RigFrameRec? {
        guard let r = lineRecipe, let key = r.frame else { return nil }
        return rigging?.frames.first { $0.key == key }
    }

    /// Splays (and frame tilt or pick point) aimed at the audience.
    public func autoSplay() {
        editLine { r in r.auto = AutoSplayRec(firstM: audienceFirst, lastM: audienceLast, listenerZ: listenerZ, rakeDeg: rakeDeg) }
        do {
            setAssembly(try store.doc.assemblyAutoSplay(assembly: assembly))
            error = nil
        } catch { self.error = "\(error)" }
    }

    /// Audience inputs changed: the drawing and the level prediction follow.
    public func refreshGuides() {
        if case .lineArray = assembly.kind {
            editLine { r in r.auto = AutoSplayRec(firstM: audienceFirst, lastM: audienceLast, listenerZ: listenerZ, rakeDeg: rakeDeg) }
        }
    }

    /// Delay and gain of the rear-cancelling (inverted) elements for the
    /// most rear rejection over the tuning band.
    public func tuneRear() {
        do {
            let t = try store.doc.assemblyTuneRear(assembly: assembly, fLo: tuneLo, fHi: tuneHi)
            setAssembly(t.assembly)
            tuneResult = t.rearMinusFrontDb
            error = nil
        } catch { self.error = "\(error)" }
    }

    // MARK: Layout and prediction

    func refresh() {
        do {
            layout = try store.doc.assemblyLayout(assembly: assembly)
            error = nil
        } catch {
            layout = nil
            self.error = "\(error)"
        }
        schedulePrediction()
    }

    private func schedulePrediction() {
        predictionTask?.cancel()
        guard predict, layout != nil else { polar = nil; distance = nil; return }
        let doc = store.doc, a = assembly, freqs = predictFrequencies, vertical = polarVertical
        let line = kind == .lineArray
        let (first, last, z, rake) = (audienceFirst, audienceLast, listenerZ, rakeDeg)
        predicting = true
        predictionTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(180))
            if Task.isCancelled { return }
            let result = await Task.detached(priority: .userInitiated) { () -> (PredictionRec?, PredictionRec?, PredictionRec?) in
                let p = try? doc.assemblyPolar(assembly: a, freqs: freqs, vertical: vertical, radiusM: 30, stepDeg: 2.5, widthOct: line ? 1.0 / 3 : 0)
                if line {
                    let d = try? doc.assemblyDistance(assembly: a, freqs: freqs, fromM: max(0.5, first * 0.5), toM: last * 1.15, z0: z, rakeDeg: rake, points: 90, widthOct: 1.0 / 3)
                    return (p, d, nil)
                }
                // Stacks: ahead of and behind the stack (the same line turned around).
                var back = a
                back.yawDeg += 180
                let d = try? doc.assemblyDistance(assembly: a, freqs: freqs, fromM: 2, toM: 30, z0: 1.2, rakeDeg: 0, points: 57, widthOct: 0)
                let r = try? doc.assemblyDistance(assembly: back, freqs: freqs, fromM: 2, toM: 30, z0: 1.2, rakeDeg: 0, points: 57, widthOct: 0)
                return (p, d, r)
            }.value
            if Task.isCancelled { return }
            self?.polar = result.0
            self?.distance = result.1
            self?.distanceRear = result.2
            self?.predicting = false
        }
    }

    // MARK: Commit

    /// Inserts the draft as a new group (or applies it to the edited group) and selects it.
    @discardableResult
    public func commit() -> Bool {
        if let g = groupID {
            let ok = ctx.edit(kind == .stack ? "Edit Stack" : "Edit Line Array") { doc in
                try doc.updateAssembly(group: g, assembly: assembly)
                if var r = store.groupsById[g], r.name != name { r.name = name; try doc.updateGroup(group: r) }
            }
            if ok { ctx.select([.group(g)]) }
            return ok
        }
        var id: UInt64 = 0
        let ok = ctx.edit(kind == .stack ? "Insert Stack" : "Insert Line Array") { doc in
            id = try doc.insertAssembly(name: name, assembly: assembly)
        }
        if ok {
            groupID = id
            ctx.select([.group(id)])
            ctx.frameSelection()
            ctx.post("Inserted \(name) · \(elementCount) speakers", .info)
        }
        return ok
    }
}
