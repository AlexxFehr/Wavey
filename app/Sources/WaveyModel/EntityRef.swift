import Foundation
import WaveyCore

/// Any selectable model object. Selection, navigation and the inspector
/// work on these, never on engine types directly.
public enum EntityRef: Hashable, Codable, Sendable {
    case surface(String)
    case segment(UInt64)
    case speaker(UInt64)
    case microphone(UInt64)
    case plane(UInt64)
    case group(UInt64)
    case layer(String)
    case level(String)
    case material(String)
    case device(String)
    /// A speaker in the stock library (its file path), not yet in the project.
    case libraryItem(String)
    case run(UInt32)

    /// Engine entity key (`spk:3`, `mic:4`, `plane:5`, `wall:7:f`, …).
    public init?(engineKey key: String) {
        let parts = key.split(separator: ":", maxSplits: 1).map(String.init)
        guard parts.count == 2 else { return nil }
        switch parts[0] {
        case "spk": guard let v = UInt64(parts[1]) else { return nil }; self = .speaker(v)
        case "mic": guard let v = UInt64(parts[1]) else { return nil }; self = .microphone(v)
        case "plane": guard let v = UInt64(parts[1]) else { return nil }; self = .plane(v)
        case "group": guard let v = UInt64(parts[1]) else { return nil }; self = .group(v)
        case "wall", "floor", "ceiling": self = .surface(key)
        default: return nil
        }
    }

    public enum Kind: String, Sendable, CaseIterable {
        case surface, segment, speaker, microphone, plane, group, layer, level, material, device, libraryItem, run
    }

    public var kind: Kind {
        switch self {
        case .surface: .surface
        case .segment: .segment
        case .speaker: .speaker
        case .microphone: .microphone
        case .plane: .plane
        case .group: .group
        case .layer: .layer
        case .level: .level
        case .material: .material
        case .device: .device
        case .libraryItem: .libraryItem
        case .run: .run
        }
    }

    public var speakerID: UInt64? { if case .speaker(let v) = self { v } else { nil } }
    public var microphoneID: UInt64? { if case .microphone(let v) = self { v } else { nil } }
    public var planeID: UInt64? { if case .plane(let v) = self { v } else { nil } }
    public var surfaceKey: String? { if case .surface(let v) = self { v } else { nil } }
    public var segmentID: UInt64? { if case .segment(let v) = self { v } else { nil } }
    public var groupID: UInt64? { if case .group(let v) = self { v } else { nil } }
    public var layerID: String? { if case .layer(let v) = self { v } else { nil } }
    public var levelID: String? { if case .level(let v) = self { v } else { nil } }
    public var materialID: String? { if case .material(let v) = self { v } else { nil } }
    public var deviceID: String? { if case .device(let v) = self { v } else { nil } }
    public var libraryPath: String? { if case .libraryItem(let v) = self { v } else { nil } }
}

extension SpeakerRec: Identifiable {}
extension GroupRec: Identifiable {}
extension MicrophoneRec: Identifiable {}
extension CutPlaneRec: Identifiable {}
