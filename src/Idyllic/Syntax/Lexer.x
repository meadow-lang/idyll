{
{-# OPTIONS_GHC -Wno-missing-signatures #-}
{-# OPTIONS_GHC -Wno-incomplete-patterns #-}
{-# OPTIONS_GHC -Wno-incomplete-uni-patterns #-}
{-# OPTIONS_GHC -Wno-unused-imports #-}
{-# OPTIONS_GHC -Wno-name-shadowing #-}
{-# OPTIONS_GHC -Wno-unused-matches #-}
{-# OPTIONS_GHC -Wno-unused-local-binds #-}
{-# OPTIONS_GHC -Wno-tabs #-}

-- | Hand-rolled Alex lexer for Idyll.
--
--   No Alex @%wrapper@ is used: the scanner interface (@AlexInput@,
--   @alexGetByte@, @alexInputPrevChar@) and the driver monad ('Lexer') are
--   defined by hand below.  The lexer is /infallible/: 'tokenize' always
--   returns a token stream together with a list of 'Diag' reports.  Every
--   token carries a 'Span' of absolute, 0-based byte offsets into the
--   UTF-8 encoded source.
module Idyllic.Syntax.Lexer
  ( Diag
  , tokenize
  , tokenizeNamed
  ) where

import Data.Bits ((.&.))
import Data.ByteString (ByteString)
import qualified Data.ByteString as BS
import Data.Char (chr)
import qualified Data.Char as Char
import Data.List (foldl')
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Word (Word8)
import Numeric (showHex)

import Error.Diagnose (Marker (..), Position (..), Report, err)

import Idyllic.Syntax.Token
import Idyllic.Utils (Located (..), Span (..))
}

$unispace   = \x05
$nonzero    = [1-9]
$digit      = [0-9]
$bindig     = [01]
$octdig     = [0-7]
$hexdig     = [0-9A-Fa-f]
$alpha      = [a-zA-Z]
$lower      = [_a-z]
$upper      = [A-Z]
$tab        = \t
$whitespace = [\ $unispace \v]
$newline    = [\n\r\f]
$identChar  = [$alpha $digit \_ \']
$opChar     = [\!\$\%\&\*\+\.\/\<\=\>\?\@\|\\\~\:\^\-\`]

$strBare    = [^ \" \\ \n \r \f \xD800-\xDFFF]
$chrBare    = [^ \' \\ \n \r \f \xD800-\xDFFF]

@lowerCaseIdent = $lower $identChar*
@upperCaseIdent = $upper $identChar*
@opIdent        = $opChar+
@conOpIdent     = ":" $opChar+

@binary      = "0b" $bindig+
@octal       = "0o" $octdig+
@hexadecimal = "0x" $hexdig+
@decimal     = ($nonzero $digit* | "0")
@int         = @binary | @octal | @hexadecimal | @decimal

-- An escape is a backslash followed by any single (non-newline) codepoint.
-- All validation of the escape body happens in 'resolveEscapes' so that we
-- can emit precise diagnostics instead of a bare lexical failure.
@esc     = \\ .
@strChar = $strBare | @esc
@chrChar = $chrBare | @esc

@string             = \" @strChar* \"
@stringUnterminated = \" @strChar*

@char             = \' @chrChar \'
@charUnterminated = \' @chrChar

idyll :-

  $tab                           ;
  $whitespace+                   ;
  "--" .*                        ;
  $newline                       { tok TokenNewline }

  "("                            { tok TokenLParen }
  ")"                            { tok TokenRParen }
  "{"                            { tok TokenLBrace }
  "}"                            { tok TokenRBrace }
  "["                            { tok TokenLBracket }
  "]"                            { tok TokenRBracket }
  "!"                            { tok TokenBang }
  "#"                            { tok TokenHash }
  "@"                            { tok TokenAt }
  \\                             { tok TokenBackSlash }
  "<-"                           { tok TokenLArrow }
  "->"                           { tok TokenRArrow }
  "=>"                           { tok TokenRFatArrow }
  "="                            { tok TokenEq }
  ":"                            { tok TokenColon }
  ";"                            { tok TokenSemi }
  ","                            { tok TokenComma }
  "."                            { tok TokenPeriod }
  "|"                            { tok TokenBar }
  "_"                            { tok TokenUnderscore }

  @lowerCaseIdent                { lowerIdentA }
  @upperCaseIdent                { upperIdentA }
  @conOpIdent                    { conOpIdentA }
  @opIdent                       { opIdentA }

  @int                           { intA }

  @char                          { charA }
  \' \'                          { emptyCharA }
  @charUnterminated              { unterminatedCharA }
  \'                             { unterminatedCharA }

  @string                        { stringA }
  @stringUnterminated             { unterminatedStringA }

{
-- ---------------------------------------------------------------------------
-- Public interface
-- ---------------------------------------------------------------------------

-- | A single diagnostic report produced while lexing.
type Diag = Report Text

-- | Lex a source 'Text', attributing diagnostics to a synthetic @\<input\>@
--   file.  Never fails: malformed input yields 'TokenError' tokens plus
--   diagnostics.
tokenize :: Text -> ([LToken], [Diag])
tokenize = tokenizeNamed "<input>"

-- | Like 'tokenize', but diagnostics reference the given file path.
tokenizeNamed :: FilePath -> Text -> ([LToken], [Diag])
tokenizeNamed file src =
  let bytes = TE.encodeUtf8 src
      st0 = LexState
              { lsInput = AlexInput
                  { aiOffset = 0
                  , aiLine   = 1
                  , aiCol    = 1
                  , aiPrev   = '\n'
                  , aiBytes  = bytes
                  }
              , lsFile  = file
              , lsDiags = []
              }
      (toks, st1) = unLexer lexLoop st0
   in (toks, reverse (lsDiags st1))

-- ---------------------------------------------------------------------------
-- Scanner input (the "basic" Alex interface, no wrapper)
-- ---------------------------------------------------------------------------

data AlexInput = AlexInput
  { aiOffset :: !Int          -- ^ absolute byte offset from start of input, 0-based
  , aiLine   :: !Int          -- ^ current line, 1-based
  , aiCol    :: !Int          -- ^ current column (in codepoints), 1-based
  , aiPrev   :: !Char         -- ^ previously consumed char, for 'alexInputPrevChar'
  , aiBytes  :: !ByteString   -- ^ remaining input
  }

-- | Position fields always describe the /next/ unconsumed byte.  Columns
--   only advance on the leading byte of a UTF-8 sequence so that multi-byte
--   codepoints count as a single column.
alexGetByte :: AlexInput -> Maybe (Word8, AlexInput)
alexGetByte ai = case BS.uncons (aiBytes ai) of
  Nothing -> Nothing
  Just (w, rest) ->
    let ch     = chr (fromIntegral w)
        isCont = w .&. 0xC0 == 0x80
        line'
          | ch == '\n' = aiLine ai + 1
          | otherwise  = aiLine ai
        col'
          | ch == '\n' = 1
          | isCont     = aiCol ai
          | otherwise  = aiCol ai + 1
     in Just ( w
             , ai { aiOffset = aiOffset ai + 1
                  , aiLine   = line'
                  , aiCol    = col'
                  , aiPrev   = ch
                  , aiBytes  = rest
                  }
             )

alexInputPrevChar :: AlexInput -> Char
alexInputPrevChar = aiPrev

-- ---------------------------------------------------------------------------
-- The lexer monad (a hand-rolled state monad)
-- ---------------------------------------------------------------------------

data LexState = LexState
  { lsInput :: !AlexInput
  , lsFile  :: !FilePath
  , lsDiags :: [Diag]         -- ^ accumulated in reverse order
  }

newtype Lexer a = Lexer { unLexer :: LexState -> (a, LexState) }

instance Functor Lexer where
  fmap f (Lexer g) = Lexer $ \s -> let (a, s') = g s in (f a, s')

instance Applicative Lexer where
  pure a = Lexer $ \s -> (a, s)
  Lexer f <*> Lexer g = Lexer $ \s ->
    let (h, s1) = f s
        (a, s2) = g s1
     in (h a, s2)

instance Monad Lexer where
  Lexer g >>= k = Lexer $ \s -> let (a, s1) = g s in unLexer (k a) s1

getInput :: Lexer AlexInput
getInput = Lexer $ \s -> (lsInput s, s)

putInput :: AlexInput -> Lexer ()
putInput i = Lexer $ \s -> ((), s { lsInput = i })

getFile :: Lexer FilePath
getFile = Lexer $ \s -> (lsFile s, s)

addDiag :: Diag -> Lexer ()
addDiag d = Lexer $ \s -> ((), s { lsDiags = d : lsDiags s })

-- ---------------------------------------------------------------------------
-- Driver
-- ---------------------------------------------------------------------------

-- | Source span of a lexeme, in codepoint-based line/column coordinates.
data Loc = Loc
  { locBeginL :: !Int
  , locBeginC :: !Int
  , locEndL   :: !Int
  , locEndC   :: !Int
  }

-- | A rule action: given the matched bytes, its byte 'Span' and its 'Loc',
--   optionally produce a token (whitespace-like rules produce 'Nothing').
type Action = ByteString -> Span -> Loc -> Lexer (Maybe LToken)

lexLoop :: Lexer [LToken]
lexLoop = do
  inp <- getInput
  case alexScan inp 0 of
    AlexEOF -> pure []
    AlexSkip inp' _ -> putInput inp' >> lexLoop
    AlexToken inp' len act -> do
      let lexeme = BS.take len (aiBytes inp)
          sp     = Span (aiOffset inp) (aiOffset inp')
          loc    = Loc (aiLine inp) (aiCol inp) (aiLine inp') (aiCol inp')
      putInput inp'
      mtok <- act lexeme sp loc
      rest <- lexLoop
      pure (maybe rest (: rest) mtok)
    AlexError _ -> do
      file <- getFile
      let inp2 = dropCodepoint inp
      if aiOffset inp2 == aiOffset inp
        then pure []   -- no progress possible; bail out
        else do
          let sp     = Span (aiOffset inp) (aiOffset inp2)
              badTxt = decodeSlice inp inp2
          addDiag $
            err Nothing
              ("unexpected character '" <> badTxt <> "'")
              [ ( pos file (aiLine inp) (aiCol inp) (aiLine inp2) (aiCol inp2)
                , This "not valid at the start of a token"
                )
              ]
              []
          putInput inp2
          rest <- lexLoop
          pure (Located TokenError sp : rest)

-- | Advance past exactly one UTF-8 codepoint (a lead byte plus any
--   continuation bytes).  Used for error recovery.
dropCodepoint :: AlexInput -> AlexInput
dropCodepoint i = case alexGetByte i of
  Nothing        -> i
  Just (_, next) -> skipCont next
  where
    skipCont j = case BS.uncons (aiBytes j) of
      Just (w, _) | w .&. 0xC0 == 0x80 ->
        case alexGetByte j of
          Just (_, j') -> skipCont j'
          Nothing      -> j
      _ -> j

-- | The bytes consumed between two inputs, decoded leniently.
decodeSlice :: AlexInput -> AlexInput -> Text
decodeSlice from to =
  TE.decodeUtf8Lenient (BS.take (aiOffset to - aiOffset from) (aiBytes from))

-- ---------------------------------------------------------------------------
-- Simple rule actions
-- ---------------------------------------------------------------------------

decode :: ByteString -> Text
decode = TE.decodeUtf8Lenient

emit :: Token -> Span -> Lexer (Maybe LToken)
emit t sp = pure (Just (Located t sp))

-- | Emit a fixed token, ignoring the matched text.
tok :: Token -> Action
tok t _ sp _ = emit t sp

lowerIdentA :: Action
lowerIdentA bs sp _ = emit (keywordOr (decode bs)) sp

keywordOr :: Text -> Token
keywordOr txt = case txt of
  "mod"   -> TokenMod
  "use"   -> TokenUse
  "data"  -> TokenData
  "type"  -> TokenType
  "let"   -> TokenLet
  "in"    -> TokenIn
  "where" -> TokenWhere
  "do"    -> TokenDo
  "case"  -> TokenCase
  "of"    -> TokenOf
  "if"    -> TokenIf
  "then"  -> TokenThen
  "else"  -> TokenElse
  _       -> TokenLowercaseIdent txt

upperIdentA :: Action
upperIdentA bs sp _ = case decode bs of
  "Type" -> emit TokenType sp        -- the sort keyword
  other  -> emit (TokenUppercaseIdent other) sp

opIdentA :: Action
opIdentA bs sp _ = emit (TokenOpIdent (decode bs)) sp

conOpIdentA :: Action
conOpIdentA bs sp _ = emit (TokenConOpIdent (decode bs)) sp

-- ---------------------------------------------------------------------------
-- Integer literals
-- ---------------------------------------------------------------------------

intA :: Action
intA bs sp _ = emit (TokenInt (parseIntLiteral (decode bs))) sp

-- | Parse an already-validated integer lexeme (any of @0b@/@0o@/@0x@ prefixed
--   or plain decimal).  Total: the regex guarantees at least one digit.
parseIntLiteral :: Text -> Integer
parseIntLiteral txt = case T.take 2 txt of
  "0b" -> parseRadix 2  (T.drop 2 txt)
  "0o" -> parseRadix 8  (T.drop 2 txt)
  "0x" -> parseRadix 16 (T.drop 2 txt)
  _    -> parseRadix 10 txt

parseRadix :: Integer -> Text -> Integer
parseRadix r = T.foldl' step 0
  where
    step acc c = acc * r + fromIntegral (Char.digitToInt c)

-- ---------------------------------------------------------------------------
-- Character literals
-- ---------------------------------------------------------------------------

charA :: Action
charA bs sp loc = do
  file <- getFile
  -- strip the surrounding quotes
  let inner = T.dropEnd 1 (T.drop 1 (decode bs))
  resolved <- resolveEscapes file loc (locBeginC loc + 1) inner
  c <- case T.unpack resolved of
    [ch] -> pure ch
    []   -> do
      addDiag $
        err Nothing "empty character literal"
          [ ( locPos file loc
            , This "a character literal must contain exactly one codepoint"
            )
          ]
          []
      pure '\xFFFD'
    (ch : _) -> do
      addDiag $
        err Nothing "character literal contains more than one codepoint"
          [ (locPos file loc, This "expected exactly one codepoint between the quotes") ]
          [ "use a string literal (\"...\") for multi-character text" ]
      pure ch
  emit (TokenChar c) sp

emptyCharA :: Action
emptyCharA _ sp loc = do
  file <- getFile
  addDiag $
    err Nothing "empty character literal"
      [ (locPos file loc, This "expected a codepoint between the quotes") ]
      []
  emit TokenError sp

unterminatedCharA :: Action
unterminatedCharA _ sp loc = do
  file <- getFile
  addDiag $
    err Nothing "unterminated character literal"
      [ (locPos file loc, This "missing the closing single quote") ]
      []
  emit TokenError sp

-- ---------------------------------------------------------------------------
-- String literals
-- ---------------------------------------------------------------------------

stringA :: Action
stringA bs sp loc = do
  file <- getFile
  let inner = T.dropEnd 1 (T.drop 1 (decode bs))
  resolved <- resolveEscapes file loc (locBeginC loc + 1) inner
  emit (TokenString resolved) sp

unterminatedStringA :: Action
unterminatedStringA _ sp loc = do
  file <- getFile
  addDiag $
    err Nothing "unterminated string literal"
      [ (locPos file loc, This "missing the closing double quote") ]
      []
  emit TokenError sp

-- ---------------------------------------------------------------------------
-- Escape resolution (shared by char and string literals)
-- ---------------------------------------------------------------------------

-- | Resolve backslash escapes in literal content, emitting a diagnostic for
--   every malformed escape and recovering by keeping the offending text.
--
--   @col0@ is the column (1-based, codepoints) of the first content
--   character, used to point diagnostics at the exact escape.
resolveEscapes :: FilePath -> Loc -> Int -> Text -> Lexer Text
resolveEscapes file loc col0 = go 0 [] . T.unpack
  where
    line = locBeginL loc

    finish acc = pure (T.pack (reverse acc))

    -- i: codepoint offset within the content, for column arithmetic
    go :: Int -> [Char] -> [Char] -> Lexer Text
    go _ acc [] = finish acc
    go i acc ('\\' : rest) = case rest of
      [] -> do
        diagAt (col0 + i) (col0 + i + 1)
          "trailing backslash in literal"
          "expected an escape character after this backslash"
          []
        finish acc
      (c : cs) -> case c of
        'n'  -> go (i + 2) ('\n' : acc) cs
        't'  -> go (i + 2) ('\t' : acc) cs
        'r'  -> go (i + 2) ('\r' : acc) cs
        '\\' -> go (i + 2) ('\\' : acc) cs
        '\'' -> go (i + 2) ('\'' : acc) cs
        '"'  -> go (i + 2) ('"'  : acc) cs
        '0'  -> go (i + 2) ('\0' : acc) cs
        'a'  -> go (i + 2) ('\a' : acc) cs
        'b'  -> go (i + 2) ('\b' : acc) cs
        'f'  -> go (i + 2) ('\f' : acc) cs
        'v'  -> go (i + 2) ('\v' : acc) cs
        'x'  -> hexEscape 'x' 2 i acc cs
        'u'  -> hexEscape 'u' 4 i acc cs
        'U'  -> hexEscape 'U' 8 i acc cs
        _    -> do
          diagAt (col0 + i) (col0 + i + 2)
            ("unknown escape sequence '\\" <> T.singleton c <> "'")
            "this escape sequence is not recognized"
            [ "supported escapes: \\n \\r \\t \\0 \\a \\b \\f \\v \\\\ \\' \\\" \\xNN \\uNNNN \\UNNNNNNNN" ]
          go (i + 2) (c : acc) cs
    go i acc (c : cs) = go (i + 1) (c : acc) cs

    -- kind: 'x' | 'u' | 'U'; maxDigits: 2 | 4 | 8
    hexEscape :: Char -> Int -> Int -> [Char] -> [Char] -> Lexer Text
    hexEscape kind maxDigits i acc cs =
      let hs      = take maxDigits (takeWhile Char.isHexDigit cs)
          n       = length hs
          cs'     = drop n cs
          escCol  = col0 + i                 -- the backslash
          endCol  = col0 + i + 2 + n         -- past the digits
       in if n == 0
            then do
              diagAt escCol (escCol + 2)
                ("'\\" <> T.singleton kind <> "' escape needs at least one hexadecimal digit")
                "expected hexadecimal digits here"
                []
              go (i + 2) acc cs
            else
              let v = foldl' (\a ch -> a * 16 + Char.digitToInt ch) 0 hs
               in if isValidScalar v
                    then go (i + 2 + n) (chr v : acc) cs'
                    else do
                      diagAt escCol endCol
                        ("invalid Unicode scalar value U+" <> T.pack (upperHex v))
                        "not a valid Unicode codepoint"
                        [ "Unicode scalar values are U+0000..U+D7FF and U+E000..U+10FFFF" ]
                      go (i + 2 + n) ('\xFFFD' : acc) cs'

    diagAt :: Int -> Int -> Text -> Text -> [Text] -> Lexer ()
    diagAt c1 c2 msg lbl hints =
      addDiag $
        err Nothing msg
          [ (pos file line c1 line c2, This lbl) ]
          hints

isValidScalar :: Int -> Bool
isValidScalar v = v <= 0x10FFFF && not (v >= 0xD800 && v <= 0xDFFF)

upperHex :: Int -> String
upperHex v = map Char.toUpper (showHex v "")

-- ---------------------------------------------------------------------------
-- Position helpers
-- ---------------------------------------------------------------------------

pos :: FilePath -> Int -> Int -> Int -> Int -> Position
pos file bl bc el ec = Position (bl, bc) (el, ec) file

locPos :: FilePath -> Loc -> Position
locPos file (Loc bl bc el ec) = Position (bl, bc) (el, ec) file
}
