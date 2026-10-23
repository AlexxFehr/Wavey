import AppKit
import Foundation
import WaveyCore
import WaveyModel
import WaveyViewport

extension WindowContext {
    /// `--selftest`: drives the tools through a full workflow on a small
    /// room (draw, extrude, place, solve, read results, undo) and exits
    /// with status 1 if any check fails.
    public func runSelfTest() {
        Task { @MainActor in await selfTest() }
    }

    private func selfTest() async {
        var failures = 0
        func check(_ ok: Bool, _ what: String) {
            print("\(ok ? "PASS" : "FAIL") \(what)")
            if !ok { failures += 1 }
        }
        // Every refresh is compared with a full read of the core.
        store.verifyMirror = true
        func pump(_ s: Double = 0.02) async { try? await Task.sleep(for: .seconds(s)) }
        func click(_ x: Double, _ y: Double, _ z: Double = 0) {
            tool.handle(.down(world: Vec3(x, y, z), pick: nil, screen: .zero, modifiers: [], clickCount: 1), in: workspace)
            tool.handle(.up(world: Vec3(x, y, z), pick: nil, screen: .zero, modifiers: []), in: workspace)
        }

        // Geometry: rectangle, extrude, height.
        workspace = .geometry
        state.flags.remove("snap.grid")
        tool.activate(.rectangle)
        click(0, 0)
        click(8, 6)
        check(store.sketch.segments.count == 4, "rectangle adds 4 edges")
        check(store.undoName == "Add Rectangle", "undo name is “Add Rectangle”")
        tool.activate(.extrude)
        click(4, 3)
        check(store.room != nil, "extrude builds a room")
        runCommand("3.2")
        check(abs((store.room?.height ?? 0) - 3.2) < 1e-9, "typed height sets the room height")
        check(store.room?.surfaces.contains { $0.code == "W-01" } ?? false, "walls get codes")
        runCommand("LINE")
        check(activeTool == .line, "command alias LINE starts the line tool")
        tool.escape()

        // Modelling: rake, balcony, vault and column on the drawn room; a patch; undo.
        let v0 = store.room?.volume ?? 0
        useTool(.rake)
        click(4, 0)
        click(8, 6)
        let rake = store.features.first
        check(rake?.label == "Rake" && rake?.error == nil && (store.room?.volume ?? v0) < v0 - 1, "rake tool takes the seating out of the air")
        useTool(.balcony)
        click(6, 0)
        click(8, 6)
        useTool(.vault)
        click(0, 0)
        click(8, 6)
        useTool(.column)
        click(2, 4.5)
        check(store.features.count == 4 && store.features.allSatisfy { $0.error == nil } && store.roomError == nil,
              "balcony, vault and column added (\(store.roomError ?? "ok"))")
        check(store.room?.surfaces.contains { $0.key.hasPrefix("feat:") && $0.kind == .floor } ?? false, "rake treads are floors")
        if let wall = store.room?.surfaces.first(where: { $0.key.hasPrefix("wall:") }) {
            useTool(.split)
            var c = SIMD3<Double>(0, 0, 0)
            let n = wall.positions.count / 3
            for i in 0..<n {
                let p: SIMD3<Double> = SIMD3(Double(wall.positions[i * 3]), Double(wall.positions[i * 3 + 1]), Double(wall.positions[i * 3 + 2]))
                c += p / Double(n)
            }
            tool.handle(.down(world: Vec3(c.x, c.y, 1.0), pick: picks.id(.surface(wall.key)), screen: .zero, modifiers: [], clickCount: 1), in: workspace)
            tool.handle(.up(world: Vec3(c.x, c.y, 1.0), pick: nil, screen: .zero, modifiers: []), in: workspace)
            check(store.features.last?.label == "Imprint" && store.room?.surfaces.contains { $0.key == "feat:\(store.features.last!.id):0" } == true,
                  "split surface adds a patch with its own key")
        }
        tool.escape()
        while !store.features.isEmpty && store.canUndo { undo() }
        check(store.features.isEmpty && abs((store.room?.volume ?? 0) - v0) < 1e-6, "undo removes the features")

        // Sources and receivers.
        workspace = .sources
        // A speaker dropped from the library (the samples are a test fixture only).
        edit("Add Devices") { try $0.addSampleSpeakerModels() }
        let omni = store.speakerModels.first { !$0.hasBalloon }?.id ?? ""
        placeDevice(omni, at: Vec3(2, 2, 0))
        check(store.speakers.count == 1, "a dropped device places a source")
        check(store.speakers.first?.code == "S1", "source code S1")
        tool.escape()
        tool.activate(.microphone)
        click(6, 4)
        check(store.microphones.count == 1 && store.microphones[0].includeInSolve, "microphone tool places a solved point")
        tool.escape()
        // Microphone line: 5 receivers as one group, one undo step.
        let line = addMicArray(MicArrayRequest(grid: false, a: Vec3(1, 1, 0), b: Vec3(7, 1, 0), at: .zero), count: 5, rows: 0, cols: 0, z: 1.2)
        check(store.microphones.count == 6 && line.flatMap { store.groupsById[$0]?.receivers.count } == 5, "microphone line adds 5 receivers in a group")
        undo()
        check(store.microphones.count == 1 && store.groups.isEmpty, "undo removes the microphone line")
        // A cut plane while laying out sources.
        useTool(.planeH)
        click(4, 3)
        check(store.cutPlanes.count == 1 && store.cutPlanes[0].axis == .horizontal, "horizontal cut plane in Setup")
        undo()
        check(store.cutPlanes.isEmpty, "undo removes the cut plane")
        tool.escape()
        if let s = store.speakers.first, let m = store.microphones.first {
            var r = s
            r.aim = .microphone(id: m.id)
            edit("Aim Source") { try $0.updateSpeaker(speaker: r) }
            let yaw = store.speakers.first?.yawDeg ?? 0
            check(abs(yaw - atan2(4 - 2, 6 - 2) * 180 / .pi) < 2, "aim target sets yaw (\(String(format: "%.1f", yaw))°)")
        }

        // Results: cut plane, small solve.
        workspace = .results
        tool.activate(.cutPlane)
        click(4, 3)
        check(store.cutPlanes.count == 1 && store.cutPlanes[0].offset == 1.2, "cut plane tool adds a horizontal plane at 1.2 m")
        tool.escape()
        var settings = store.simSettings
        settings.fMin = 63
        settings.fMax = 250
        settings.octaveFraction = 3
        settings.maxOrder = 2
        settings.lateReverb = false
        settings.fdtdEnabled = false
        settings.diffraction = false
        edit("Change Solve Settings") { try $0.setSimSettings(settings: settings) }
        check(store.blockingIssue == nil, "no blocking issues (\(store.blockingIssue?.message ?? "none"))")
        runs.solve()
        let t0 = Date()
        var last = ""
        while runs.solving && Date().timeIntervalSince(t0) < 60 {
            await pump(0.1)
            let now = "\(Int(runs.fraction * 100))% \(runs.stage)"
            if now != last { print("  … \(now)"); last = now }
        }
        check(!runs.solving && store.runs.count == 1, "solve finishes with one run (\(String(format: "%.1f", Date().timeIntervalSince(t0))) s)")
        if case .current = runs.status { check(true, "run status current") } else { check(false, "run status current") }
        setFrequency(125)
        var stack: PlaneStack?
        for _ in 0..<200 where stack == nil { stack = cache.stack(store.cutPlanes[0].id); await pump() }
        check(stack != nil && (stack?.valuesDb.contains { $0.isFinite } ?? false), "plane stack arrives with finite values")
        var level: Double?
        for _ in 0..<200 where level == nil { level = micLevel(store.microphones[0].id); await pump() }
        check(level.map { $0 > 40 && $0 < 140 } ?? false, "mic level is plausible (\(level.map { String(format: "%.1f dB", $0) } ?? "nil"))")
        if let p = store.cutPlanes.first, let f = fieldValues(p) {
            check(f.range.upperBound > f.range.lowerBound, "field range is set")
        }
        // Moving the receiver makes the run outdated.
        if var m = store.microphones.first {
            m.position.x += 0.5
            edit("Move Receiver") { try $0.updateMicrophone(mic: m) }
            if case .outdated = runs.status { check(true, "moving a solved receiver marks the run outdated") } else { check(false, "outdated after edit") }
            // The source is aimed at this receiver, so it turns with it.
            check(Set(store.changesSinceRun) == ["receivers", "sources"], "changes since run: \(store.changesSinceRun)")
            undo()
            if case .current = runs.status { check(true, "undo makes the run current again") } else { check(false, "current after undo") }
        }
        // Import a room model (millimetres, two material groups), solve it, undo.
        let obj = """
            v 0 0 0\nv 8000 0 0\nv 8000 6000 0\nv 0 6000 0\nv 0 0 3000\nv 8000 0 3000\nv 8000 6000 3000\nv 0 6000 3000
            usemtl Concrete
            f 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 3 4 8 7\nf 1 5 8 4
            usemtl Window_Glass
            f 2 3 7 6
            """
        let objURL = FileManager.default.temporaryDirectory.appendingPathComponent("wavey-selftest-room.obj")
        try? obj.replacingOccurrences(of: "\\n", with: "\n").write(to: objURL, atomically: true, encoding: .utf8)
        if let p = try? store.doc.previewImport(path: objURL.path, options: nil) {
            check(p.options.scale == 0.001 && p.report.watertight && p.groups.count == 2, "import preview: mm, closed, 2 groups")
            check(p.groups.first { $0.name == "Window_Glass" }?.materialId == "glass-window", "import guesses glass for Window_Glass")
            edit("Import Room Model") { _ = try $0.importRoom(path: objURL.path, options: p.options, groups: p.groups) }
            check(store.roomModel != nil && abs((store.room?.volume ?? 0) - 144) < 1e-6, "imported room: 8 × 6 × 3 m")
            settings.diffraction = true
            edit("Change Solve Settings") { try $0.setSimSettings(settings: settings) }
            // An audience area over all floors, seated.
            var area: UInt64 = 0
            edit("Add Audience Area") { area = try $0.addAudienceArea(surfaceKeys: [], height: 1.2, spacing: 0.5) }
            if let p = store.planesById[area] {
                let pts = audiencePoints(p)
                check(pts.count > 300 && stride(from: 2, to: pts.count, by: 3).allSatisfy { abs(pts[$0] - 1.2) < 1e-4 }, "audience area points at 1.2 m above the floor")
            }
            runs.solve()
            let t1 = Date()
            while runs.solving && Date().timeIntervalSince(t1) < 60 { await pump(0.1) }
            check(!runs.solving && store.runs.count == 2, "imported room solves (\(String(format: "%.1f", Date().timeIntervalSince(t1))) s)")
            var draped: PlaneStack?
            for _ in 0..<200 where draped == nil { draped = cache.stack(area); await pump() }
            check(draped.map { $0.heights.count == $0.cells } ?? false, "audience area map carries its heights")
            undo()
            undo()
            undo()
            check(store.roomModel == nil && abs((store.room?.height ?? 0) - 3.2) < 1e-9, "undo restores the drawn room")
        } else {
            check(false, "import preview reads the OBJ")
        }

        // Undo everything.
        var n = 0
        while store.canUndo && n < 100 { undo(); n += 1 }
        check(store.sketch.segments.isEmpty && store.speakers.isEmpty, "undo all returns to an empty model (\(n) steps)")
        // Open air: no walls, a source and a microphone. The ground reflects; free field does not.
        workspace = .sources
        check(store.isOpenAir, "a project without walls is open air")
        edit("Add Devices") { try $0.addSampleSpeakerModels() }
        let openOmni = store.speakerModels.first { !$0.hasBalloon }?.id ?? ""
        placeDevice(openOmni, at: Vec3(0, 0, 0))
        tool.escape()
        edit("Add Receiver") { _ = $0.addMicrophone(position: Point3(x: 10, y: 0, z: 1.5)) }
        var open = store.simSettings
        open.fMin = 63
        open.fMax = 250
        open.octaveFraction = 3
        open.fdtdEnabled = false
        edit("Change Solve Settings") { try $0.setSimSettings(settings: open) }
        check(store.blockingIssue == nil, "open air has no blocking issue (\(store.blockingIssue?.message ?? "none"))")
        let runsBefore = store.runs.count
        runs.solve()
        let t2 = Date()
        while runs.solving && Date().timeIntervalSince(t2) < 60 { await pump(0.1) }
        check(!runs.solving && store.runs.count == runsBefore + 1, "open air solves over the ground (\(runs.lastError ?? "no error"))")
        edit("Set Ground") { try $0.setGroundMaterial(id: "grass") }
        check(store.groundMaterial == "grass" && store.activeRun?.stale == true, "choosing a ground makes the run outdated")
        open.freeField = true
        edit("Change Solve Settings") { try $0.setSimSettings(settings: open) }
        runs.solve()
        let t3 = Date()
        while runs.solving && Date().timeIntervalSince(t3) < 60 { await pump(0.1) }
        check(!runs.solving && store.runs.count == runsBefore + 2 && store.simSettings.freeField, "free field solves as a second run")
        edit("Set Ground") { try $0.setGroundMaterial(id: "asphalt") }
        check(store.activeRun?.stale == false, "free field does not depend on the ground")
        check(store.mirrorMismatches.isEmpty, "incremental refresh matches a full read\(store.mirrorMismatches.isEmpty ? "" : ": \(Set(store.mirrorMismatches).sorted())")")
        print(failures == 0 ? "SELFTEST OK" : "SELFTEST FAILED (\(failures))")
        exit(failures == 0 ? 0 : 1)
    }
}
