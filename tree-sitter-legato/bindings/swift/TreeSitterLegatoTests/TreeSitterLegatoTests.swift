import XCTest
import SwiftTreeSitter
import TreeSitterLegato

final class TreeSitterLegatoTests: XCTestCase {
    func testCanLoadGrammar() throws {
        let parser = Parser()
        let language = Language(language: tree_sitter_legato())
        XCTAssertNoThrow(try parser.setLanguage(language),
                         "Error loading Legato grammar")
    }
}
