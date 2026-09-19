import Foundation

// Adapted from opencc-js 1.4.1, Copyright (c) 2020-2021 The nk2028 Project, MIT.
// See Resources/OpenCC/MIT.txt and README.md for code and dictionary provenance.
actor ChineseScriptConverter {
    static let shared = ChineseScriptConverter()
    private var dictionaries: [String: [Entry]] = [:]
    private var pipelines: [ChineseScriptPreference: Pipeline] = [:]
    private var normalization: ScalarTrie?

    func convert(_ text: String, preferences: TranscriptionPreferences) throws -> String {
        try Task.checkCancellation()
        guard let target = preferences.target(for: text), !text.isEmpty,
              text.range(of: "\\p{script=Han}", options: .regularExpression) != nil else { return text }
        let pipeline: Pipeline
        if let cached = pipelines[target] {
            pipeline = cached
        } else {
            pipeline = try makePipeline(target)
            pipelines[target] = pipeline
        }
        guard let normalization else { throw ConversionError.missingResource }
        let normalized = try normalization.convert(Array(text.unicodeScalars))
        var segments = try pipeline.segmentation.segment(normalized)
        for stage in pipeline.stages {
            segments = try segments.map { try stage.convert($0) }
        }
        try Task.checkCancellation()
        return String(String.UnicodeScalarView(segments.flatMap { $0 }))
    }

    private func makePipeline(_ target: ChineseScriptPreference) throws -> Pipeline {
        defer { dictionaries.removeAll() }
        if normalization == nil { normalization = try trie(["CJK_Compatibility_Ideographs"]) }
        switch target {
        case .traditional:
            return try Pipeline(
                segmentation: trie(["STPhrases", "STPhrases_GeneratedFromRegionalPhrases"]),
                stages: [trie(["STPhrases", "STPhrases_GeneratedFromRegionalPhrases", "STCharacters"]),
                         trie(["TWPhrases", "TWVariantsPhrases", "TWVariants"])]
            )
        case .simplified:
            return try Pipeline(
                segmentation: trie(["TSPhrases"]),
                stages: [trie(["TWPhrasesRev", "TWVariantsRevPhrases", "TWVariantsRev"]),
                         trie(["TSPhrases", "TSCharacters"])]
            )
        case .asTranscribed: throw ConversionError.invalidDictionary
        }
    }

    private func trie(_ names: [String]) throws -> ScalarTrie {
        var entries: [Entry] = []
        for name in names.reversed() {
            if let cached = dictionaries[name] {
                entries.append(contentsOf: cached)
                continue
            }
            guard let url = Bundle.module.url(forResource: name, withExtension: "txt", subdirectory: "Resources/OpenCC") else {
                throw ConversionError.missingResource
            }
            let source = try String(contentsOf: url, encoding: .utf8)
            var parsed: [Entry] = []
            for (index, line) in source.split(separator: "\n").enumerated() {
                if index % 512 == 0 { try Task.checkCancellation() }
                let line = line.trimmingCharacters(in: .whitespacesAndNewlines)
                if line.isEmpty || line.hasPrefix("#") { continue }
                let columns = line.split(separator: "\t", omittingEmptySubsequences: false)
                guard columns.count == 2, !columns[0].isEmpty,
                      let first = columns[1].split(separator: " ", omittingEmptySubsequences: false).first,
                      !first.isEmpty else { throw ConversionError.invalidDictionary }
                let key = Array(columns[0].unicodeScalars)
                let value = Array(first.unicodeScalars)
                // JS filters by UTF-16 length, preserving identity phrases and astral identities.
                if key == value && columns[0].utf16.count == 1 { continue }
                parsed.append(Entry(key: key, value: value))
            }
            dictionaries[name] = parsed
            entries.append(contentsOf: parsed)
        }
        return try ScalarTrie(entries: entries)
    }

    private enum ConversionError: Error { case missingResource, invalidDictionary }
    private struct Entry: Sendable {
        let key: [Unicode.Scalar]
        let value: [Unicode.Scalar]
    }
    private struct Pipeline {
        let segmentation: ScalarTrie
        let stages: [ScalarTrie]
    }

    private struct ScalarTrie {
        private struct Node {
            var children: [UInt32: Int] = [:]
            var value: [Unicode.Scalar]?
        }
        private let nodes: [Node]

        init(entries: [Entry]) throws {
            var nodes = [Node()]
            for (index, entry) in entries.enumerated() {
                if index % 512 == 0 { try Task.checkCancellation() }
                var current = 0
                for scalar in entry.key {
                    if let next = nodes[current].children[scalar.value] {
                        current = next
                    } else {
                        let next = nodes.count
                        nodes.append(Node())
                        nodes[current].children[scalar.value] = next
                        current = next
                    }
                }
                nodes[current].value = entry.value
            }
            self.nodes = nodes
        }

        private func match(_ input: [Unicode.Scalar], at start: Int) -> (end: Int, value: [Unicode.Scalar])? {
            var current = 0
            var result: (end: Int, value: [Unicode.Scalar])?
            var index = start
            while index < input.count, let next = nodes[current].children[input[index].value] {
                current = next
                index += 1
                if let value = nodes[current].value { result = (index, value) }
            }
            return result
        }

        func convert(_ input: [Unicode.Scalar]) throws -> [Unicode.Scalar] {
            var output: [Unicode.Scalar] = []
            output.reserveCapacity(input.count)
            var index = 0
            var checkedIndex = 0
            let unmatchedEnds = try unmatchedEnds(input)
            while index < input.count {
                if index - checkedIndex >= 512 { try Task.checkCancellation(); checkedIndex = index }
                if let match = match(input, at: index) {
                    output.append(contentsOf: match.value)
                    index = match.end
                } else {
                    let end = unmatchedEnds[index]
                    output.append(contentsOf: input[index..<end])
                    index = end
                }
            }
            return output
        }

        func segment(_ input: [Unicode.Scalar]) throws -> [[Unicode.Scalar]] {
            var segments: [[Unicode.Scalar]] = []
            var unmatchedStart: Int?
            var index = 0
            var checkedIndex = 0
            let unmatchedEnds = try unmatchedEnds(input)
            while index < input.count {
                if index - checkedIndex >= 512 { try Task.checkCancellation(); checkedIndex = index }
                if let match = match(input, at: index) {
                    if let start = unmatchedStart { segments.append(Array(input[start..<index])) }
                    unmatchedStart = nil
                    segments.append(Array(input[index..<match.end]))
                    index = match.end
                } else {
                    if unmatchedStart == nil { unmatchedStart = index }
                    index = unmatchedEnds[index]
                }
            }
            if let start = unmatchedStart { segments.append(Array(input[start..<input.count])) }
            return segments
        }

        // Resolve nested descriptions from the end, preserving JS's incomplete-child fallback.
        private func unmatchedEnds(_ input: [Unicode.Scalar]) throws -> [Int] {
            var ends = Array(repeating: 0, count: input.count)
            for index in input.indices.reversed() {
                if index % 512 == 0 { try Task.checkCancellation() }
                var end = index + 1
                let count = arity(input[index].value)
                if count > 0 {
                    for _ in 0..<count {
                        guard end < input.count else { end = index + 1; break }
                        end = ends[end]
                    }
                }
                ends[index] = end
            }
            return ends
        }

        private func arity(_ scalar: UInt32) -> Int {
            if (0x2ff0...0x2ff1).contains(scalar) { return 2 }
            if (0x2ff2...0x2ff3).contains(scalar) { return 3 }
            if (0x2ff4...0x2fff).contains(scalar) { return 2 }
            return 0
        }
    }
}
