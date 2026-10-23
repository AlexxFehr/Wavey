import Foundation
import Observation
@preconcurrency import WaveyCore

/// Solve control: starts a run on a snapshot of the model, reports
/// progress at ≤ 10 Hz, cancels, and records failures. Edits during a solve
/// do not affect it; they make the finished run outdated.
@MainActor @Observable
public final class RunCoordinator {
    public enum Status: Equatable {
        case none
        case current(RunRec)
        case outdated(RunRec)
        case solving(fraction: Double, eta: TimeInterval?, stage: String)
        case failed(String)
    }

    public private(set) var solving = false
    public private(set) var fraction = 0.0
    public private(set) var stage = ""
    public private(set) var eta: TimeInterval?
    public private(set) var lastError: String?
    public private(set) var log: [String] = []
    public private(set) var startedAt: Date?
    /// Revision of the document when the running solve started.
    public private(set) var startRevision = 0

    private let store: DocumentStore
    private var job: SimulationJob?
    private var poll: Task<Void, Never>?

    public init(store: DocumentStore) {
        self.store = store
    }

    public var status: Status {
        if solving { return .solving(fraction: fraction, eta: eta, stage: stage) }
        if let lastError { return .failed(lastError) }
        guard let run = store.activeRun else { return .none }
        return run.stale ? .outdated(run) : .current(run)
    }

    /// Edits made since the running solve started.
    public var editedDuringSolve: Bool { solving && store.revision > startRevision }

    public func solve() {
        guard !solving else { return }
        if let block = store.blockingIssue {
            lastError = block.message
            return
        }
        do {
            let j = try store.doc.startSimulation()
            job = j
            solving = true
            lastError = nil
            fraction = 0
            stage = "starting"
            eta = nil
            log = []
            startedAt = Date()
            startRevision = store.revision
            poll = Task { [weak self] in
                while !Task.isCancelled {
                    try? await Task.sleep(for: .milliseconds(100))
                    guard let self, self.tick(j) else { return }
                }
            }
        } catch {
            lastError = describe(error)
        }
    }

    public func cancel() {
        job?.cancel()
    }

    public func clearError() { lastError = nil }

    /// Returns false when the job has finished.
    private func tick(_ j: SimulationJob) -> Bool {
        let p = j.progress()
        if p.stage != stage { log.append(p.stage) }
        stage = p.stage
        fraction = p.fraction
        eta = p.secondsLeft
        guard p.finished else { return true }
        solving = false
        job = nil
        let elapsed = startedAt.map { Date().timeIntervalSince($0) } ?? 0
        if let e = p.error {
            if e != "simulation cancelled" {
                lastError = e
                store.onMessage?("Solve failed: \(e)", .error)
            } else {
                store.onMessage?("Solve cancelled", .info)
            }
        } else {
            store.refresh()
            let name = store.activeRun?.name ?? "Run"
            store.onMessage?("\(name) finished · \(formatDuration(elapsed))", .info)
        }
        store.refresh()
        return false
    }
}

/// "6 m 41 s", "12 s".
public func formatDuration(_ s: TimeInterval) -> String {
    let t = Int(s.rounded())
    if t >= 3600 { return "\(t / 3600) h \((t % 3600) / 60) m" }
    if t >= 60 { return "\(t / 60) m \(t % 60) s" }
    return "\(t) s"
}
