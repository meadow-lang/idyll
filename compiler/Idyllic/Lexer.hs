module Idyllic.Lexer (tokenize) where

import Control.Applicative (empty, liftA, optional, (<|>))
import Control.Monad (void)
import Data.Text (Text, pack, unpack)
import qualified Data.Text as T
import Data.Void (Void)
import Idyllic.Token (LToken, Token (..))
import Idyllic.Utils (Located (..), Span (..))
import Text.Megaparsec
  ( MonadParsec (eof, getParserState, lookAhead, notFollowedBy, takeWhile1P, takeWhileP, token, try),
    ParseErrorBundle,
    Parsec,
    choice,
    getOffset,
    many,
    manyTill,
    oneOf,
    option,
    parse,
    some,
    (<?>),
  )
import Text.Megaparsec.Char (alphaNumChar, char, char', lowerChar, space1, string, upperChar)
import qualified Text.Megaparsec.Char.Lexer as L

type Lexer = Parsec Void Text

tokenize :: Text -> Either (ParseErrorBundle Text Void) [LToken]
tokenize = parse (sc *> some (located tokenL <* sc)) ""

tokenL :: Lexer Token
tokenL =
  choice
    [ upperCaseIdent,
      lowerCaseIdent,
      conOpIdent,
      opIdent,
      int,
      str,
      charT,
      TokenLParen <$ char '(',
      TokenRParen <$ char ')',
      TokenLBrace <$ char '{',
      TokenRBrace <$ char '}',
      TokenLBracket <$ char '[',
      TokenRBracket <$ char ']',
      TokenBang <$ char '!',
      TokenRArrow <$ string "->",
      TokenBackSlash <$ char '\\',
      TokenColon <$ char ':',
      TokenSemi <$ char ';',
      TokenComma <$ char ',',
      TokenPeriod <$ char '.',
      TokenRFatArrow <$ string "=>",
      TokenEq <$ char '=',
      TokenLArrow <$ string "<-",
      TokenBar <$ char '|',
      TokenUnderscore <$ char '_'
    ]

lowerCaseIdent :: Lexer Token
lowerCaseIdent = try $ do
  name <- pack <$> ((:) <$> identStartChar <*> many identChar)
  pure $ TokenLowercaseIdent name
  where
    identStartChar = lowerChar <|> char '_'
    identChar = alphaNumChar <|> char '_' <|> char '\''

upperCaseIdent :: Lexer Token
upperCaseIdent = TokenUppercaseIdent . pack <$> ((:) <$> upperChar <*> many alphaNumChar)

opIdent :: Lexer Token
opIdent = try $ do
  sym <- choice [startSpecial, startNotEq] <* notFollowedBy (oneOf ['=', '.', '@', '|', ':', '-'])
  if sym `elem` reservedSymbols
    then fail $ "symbol " ++ unpack sym ++ " cannot be used in place of identifier"
    else pure $ TokenOpIdent sym
  where
    opStartChar = oneOf ("!$%&*+/<>?~" ++ ['^' .. '`'] :: String)
    startSpecial = try $ T.cons <$> oneOf ['=', '.', '@', '|', ':'] <*> takeWhile1P Nothing isOpChar
    startNotEq = T.cons <$> opStartChar <*> takeWhileP Nothing isOpChar

    reservedSymbols :: [Text]
    reservedSymbols =
      [ "->",
        "=>",
        "<-",
        "!"
      ]

conOpIdent :: Lexer Token
conOpIdent = try $ TokenConOpIdent <$> (T.cons <$> char ':' <*> takeWhile1P Nothing isOpChar)

isOpChar :: Char -> Bool
isOpChar c = c `elem` ("!$%&*+./<=>?@|\\~:" ++ ['^' .. '`'] :: String)

newline :: Lexer Token
newline = TokenNewline <$ oneOf ['\n', '\r']

whitespace :: Lexer Token
whitespace = TokenWhitespace <$ takeWhile1P (Just "whitespace") isSpace
  where
    isSpace c = c == ' ' || c == '\t'

lineComment :: Lexer ()
lineComment = L.skipLineComment "--"

binary :: Lexer Integer
binary = try $ char '0' *> char' 'b' *> L.binary

octal :: Lexer Integer
octal = try $ char '0' *> char' 'o' *> L.octal

hexadecimal :: Lexer Integer
hexadecimal = try $ char '0' *> char' 'x' *> L.hexadecimal

int :: Lexer Token
int = TokenInt <$> choice [octal, hexadecimal, binary, L.decimal]

str :: Lexer Token
str = TokenString <$> (char '\"' *> (pack <$> manyTill L.charLiteral (char '\"')))

charT :: Lexer Token
charT = TokenChar <$> (char '\'' *> L.charLiteral <* char '\'')

sc :: Lexer ()
sc = L.space space1 (L.skipLineComment "--") (L.skipBlockComment "{-" "-}")

located :: Lexer a -> Lexer (Located a)
located l = do
  start <- getOffset
  res <- l
  Located res . Span start <$> getOffset

-- data TokenStream = TokenStream {lexer :: Lexer LToken, left :: ~TokenStream, right :: ~TokenStream}
