import Foundation

// Ported from src/utils/correctionLearner.js. Never learn generated additions or broad rewrites.
enum CorrectionMatcher {
    private static let common = Set("""
    the and for not with you this but his from they say her she will one all would
    there their what out about who get which when make can like time just him know
    take into year your good some could them see other than then now look only come
    over think also back after use two how our work first well way even new want
    because any these give day most are was were been has had did does said went
    made got came took saw knew thought where why here very much many still too again
    off down never every own same another both each few more less last next while
    before through under between should might must being have that its yes okay
    """.split(whereSeparator: \.isWhitespace).map(String.init))

    static func extract(original: String, field: String) -> [String] {
        guard !original.isEmpty, !field.isEmpty, original != field else { return [] }
        let region = editedRegion(original, field)
        guard region != original else { return [] }
        let before = tokenize(original), after = tokenize(region)
        guard !before.isEmpty, !after.isEmpty else { return [] }
        let pairs = substitutions(before, after)
        guard Double(pairs.count) <= Double(before.count) * 0.5 else { return [] }
        var seen = Set<String>()
        return pairs.compactMap { old, new in
            let key = new.lowercased()
            guard old.lowercased() != key, new.utf16.count >= 3, !common.contains(key),
                  !seen.contains(key), Double(distance(old.lowercased(), key)) / Double(max(old.utf16.count, new.utf16.count)) <= 0.65 else { return nil }
            seen.insert(key)
            return new
        }
    }

    private static func tokenize(_ text: String) -> [String] {
        text.split(whereSeparator: \.isWhitespace).map {
            String($0).replacingOccurrences(of: #"^[^\p{L}\p{N}_]+|[^\p{L}\p{N}_]+$"#, with: "", options: .regularExpression)
        }.filter { !$0.isEmpty }
    }
    private static func editedRegion(_ original: String, _ field: String) -> String {
        if Double(field.utf16.count) <= Double(original.utf16.count) * 1.5 { return field }
        if field.contains(original) { return original }
        let before = tokenize(original), after = tokenize(field)
        guard !before.isEmpty, after.count > before.count else { return field }
        guard before.count <= 4000, after.count <= 4000, before.count * after.count <= 4_000_000 else { return field }
        var start = 0, score = -1
        for i in 0...(after.count - before.count) {
            if Task.isCancelled { return original }
            let matches = before.indices.filter { before[$0].lowercased() == after[i + $0].lowercased() }.count
            if matches > score { start = i; score = matches }
        }
        return Double(score) >= Double(before.count) * 0.3 ? after[start..<(start + before.count)].joined(separator: " ") : field
    }
    private static func substitutions(_ before: [String], _ after: [String]) -> [(String, String)] {
        let m = before.count, n = after.count
        // Monitoring is best effort. Bound its quadratic work; long Dictation itself stays unlimited.
        guard m <= 4000, n <= 4000, m * n <= 4_000_000 else { return [] }
        var dp = [Int32](repeating: 0, count: (m + 1) * (n + 1))
        let a = before.map { $0.lowercased() }, b = after.map { $0.lowercased() }
        for i in 1...m {
            if Task.isCancelled { return [] }
            for j in 1...n { dp[i * (n + 1) + j] = a[i - 1] == b[j - 1] ? dp[(i - 1) * (n + 1) + j - 1] + 1 : max(dp[(i - 1) * (n + 1) + j], dp[i * (n + 1) + j - 1]) }
        }
        var aligned: [(String?, String?)] = []
        var i = m, j = n
        while i > 0 || j > 0 {
            if i > 0, j > 0, a[i - 1] == b[j - 1] { aligned.append((before[i - 1], after[j - 1])); i -= 1; j -= 1 }
            else if j > 0, i == 0 || dp[i * (n + 1) + j - 1] >= dp[(i - 1) * (n + 1) + j] { aligned.append((nil, after[j - 1])); j -= 1 }
            else { aligned.append((before[i - 1], nil)); i -= 1 }
        }
        aligned.reverse()
        guard aligned.count > 1 else { return [] }
        return (0..<(aligned.count - 1)).compactMap { k in
            guard let old = aligned[k].0, aligned[k].1 == nil, aligned[k + 1].0 == nil, let new = aligned[k + 1].1 else { return nil }
            return (old, new)
        }
    }
    private static func distance(_ a: String, _ b: String) -> Int {
        var a = ArraySlice(a.utf16), b = ArraySlice(b.utf16)
        while a.first != nil, a.first == b.first { a.removeFirst(); b.removeFirst() }
        while a.last != nil, a.last == b.last { a.removeLast(); b.removeLast() }
        guard a.count <= 4_000_000 / max(1, b.count) else { return Int.max }
        let leftUnits = Array(a), rightUnits = Array(b)
        var row = Array(0...rightUnits.count)
        for (i, left) in leftUnits.enumerated() {
            if Task.isCancelled { return Int.max }
            var next = [i + 1] + [Int](repeating: 0, count: rightUnits.count)
            for (j, right) in rightUnits.enumerated() { next[j + 1] = left == right ? row[j] : 1 + min(row[j], row[j + 1], next[j]) }
            row = next
        }
        return row[rightUnits.count]
    }
}
