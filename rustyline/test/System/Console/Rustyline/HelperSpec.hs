module System.Console.Rustyline.HelperSpec (spec) where

import Data.Text (Text)
import System.Console.Rustyline.Completion
import System.Console.Rustyline.Context
import System.Console.Rustyline.Highlight
import System.Console.Rustyline.Hint
import System.Console.Rustyline.History
import System.Console.Rustyline.Internal.Text (lenB)
import System.Console.Rustyline.Validate
import Test.Hspec

lcp :: [Text] -> Maybe Text
lcp = longestCommonPrefix . map candidate

spec :: Spec
spec = do
  describe "completion" $ do
    it "extract_word" $ do
      let line1 = "ls '/usr/local/b"
      extractWord line1 (lenB line1) (Just '\\') defaultBreakChars `shouldBe` (4, "/usr/local/b")
      let line2 = "ls /User\\ Information"
      extractWord line2 (lenB line2) (Just '\\') defaultBreakChars `shouldBe` (3, "/User\\ Information")

    it "unescape" $ do
      unescape "/usr/local/b" (Just '\\') `shouldBe` "/usr/local/b"
      unescape "/User\\ Information" (Just '\\') `shouldBe` "/User Information"

    it "escape" $ do
      escape "/usr/local/b" (Just '\\') defaultBreakChars NoQuote `shouldBe` "/usr/local/b"
      escape "/User Information" (Just '\\') defaultBreakChars NoQuote `shouldBe` "/User\\ Information"

    it "longest_common_prefix" $ do
      lcp [] `shouldBe` Nothing
      lcp ["User"] `shouldBe` Just "User"
      lcp ["User", "Users"] `shouldBe` Just "User"
      lcp ["User", "Users", ""] `shouldBe` Nothing
      lcp ["fée", "fête"] `shouldBe` Just "f"

    it "find_unclosed_quote" $ do
      findUnclosedQuote "ls /etc" `shouldBe` Nothing
      findUnclosedQuote "ls \"User Information" `shouldBe` Just (3, Double)
      findUnclosedQuote "ls \"/User Information\" /etc" `shouldBe` Nothing
      findUnclosedQuote "\"c:\\users\\All Users\\" `shouldBe` Just (0, Double)

    it "completes paths in this package" $ do
      (start, pairs) <- completePath "cat rustyline.ca" 16
      start `shouldBe` 4
      pairs `shouldBe` [Pair "rustyline.cabal" "rustyline.cabal"]
      (_, dirs) <- completePath "ls sr" 5
      dirs `shouldBe` [Pair "src" "src/"]

  describe "highlight" $ do
    it "find_matching_bracket" $ do
      findMatchingBracket "(..." 0 '(' `shouldBe` Nothing
      findMatchingBracket "...)" 3 ')' `shouldBe` Nothing
      findMatchingBracket "().." 0 '(' `shouldBe` Just (')', 1)
      findMatchingBracket "(..)" 0 '(' `shouldBe` Just (')', 3)
      findMatchingBracket "..()" 3 ')' `shouldBe` Just ('(', 2)
      findMatchingBracket "(..)" 3 ')' `shouldBe` Just ('(', 0)
      findMatchingBracket "(())" 0 '(' `shouldBe` Just (')', 3)
      findMatchingBracket "(())" 3 ')' `shouldBe` Just ('(', 0)

    it "check_bracket" $ do
      checkBracket ")..." 0 `shouldBe` Nothing
      checkBracket "(..." 2 `shouldBe` Nothing
      checkBracket "...(" 3 `shouldBe` Nothing
      checkBracket "...(" 4 `shouldBe` Nothing
      checkBracket "..)." 4 `shouldBe` Nothing
      checkBracket "(..." 0 `shouldBe` Just ('(', 0)
      checkBracket "(..." 1 `shouldBe` Just ('(', 0)
      checkBracket "...)" 3 `shouldBe` Just (')', 3)
      checkBracket "...)" 4 `shouldBe` Just (')', 3)

    it "matching_bracket" $ do
      matchingBracket '(' `shouldBe` ')'
      matchingBracket ')' `shouldBe` '('
      isOpenBracket '(' `shouldBe` True
      isCloseBracket ')' `shouldBe` True

    it "highlights the matching bracket" $ do
      h <- newMatchingBracketHighlighter
      highlightChar h "(a)" 3 OtherKind `shouldReturn` True
      highlight h "(a)" 3 `shouldReturn` "\x1b[1;34m(\x1b[0ma)"
      highlightChar h "(a)" 3 ForcedRefresh `shouldReturn` False
      highlight h "(a)" 3 `shouldReturn` "(a)"

  describe "hint" $ do
    it "empty_history" $ do
      history <- newMemHistory
      ctx <- newContext history
      historyHinter "test" 4 ctx `shouldReturn` Nothing

    it "suggests from history" $ do
      history <- newMemHistory
      _ <- historyAdd history "hello world"
      ctx <- newContext history
      historyHinter "hel" 3 ctx `shouldReturn` Just (hintText "lo world")
      historyHinter "hello world" 11 ctx `shouldReturn` Nothing

  describe "validate" $ do
    it "matching brackets" $ do
      validateBrackets "(a [b] {c})" `shouldBe` Valid Nothing
      validateBrackets "(a [b" `shouldBe` Incomplete
      validateBrackets "(a]" `shouldBe` Invalid (Just "Mismatched brackets: '(' is not properly closed")
      validateBrackets "a)" `shouldBe` Invalid (Just "Mismatched brackets: ')' is unpaired")
