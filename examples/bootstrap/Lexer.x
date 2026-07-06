{
module Idyllic.Syntax.Lexer (tokenize) where

import Idyllic.Syntax.Token
import Data.ByteString.Lazy (ByteString)
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text.Encoding as TE
import Data.Text (Text, stripPrefix)
import qualified Data.Text as T
import qualified Data.Char as Char
import Idyllic.Utils
import Data.Maybe (fromMaybe)
import Control.Monad.State.Strict
import Error.Diagnose 
}

-- %wrapper "posn-bytestring"

$unispace   = \x05
$nonzero    = [1-9]
$digit      = [0-9]
$bindig     = [01]
$octdig     = [0-7]
$hexdig     = [0-9A-Fa-f]      
$alpha      = [a-zA-Z]
$lower      = [_a-z]
$upper      = [A-Z]
$nonWhite   = [^$white]
$tab        = \t
$whitespace = [\ $unispace\v]
$newline    = [\n\r\f]
$identChar  = [$alpha $digit \_ \']
$opChar     = [\!\$\%\&\*\+\.\/\<\=\>\?\@\|\\\~\:\^-\`]

$strBare    = [^\"\\\n\xD800-\xDFFF]
$bareScalar = [^\'\\\n\xD800-\xDFFF]
$escSimple  = [0\'\"\\nrtabfv] 

@escByte = x ($hexdig | $hexdig $hexdig)

@u4      = $hexdig $hexdig $hexdig $hexdig
@u8      = @u4 @u4
@escUni4 = u @u4
@escUni8 = U @u8

@lowerCaseIdent = $lower $identChar*
@upperCaseIdent = $upper $identChar*
@opIdent        = $opChar+
@conOpIdent     = ":" $opChar+

@binary      = "0b" $bindig+
@octal       = "0o" $octdig+
@hexadecimal = "0x" $hexdig+
@decimal     = ($nonzero $digit* | "0")
@int         = @binary | @octal | @hexadecimal | @decimal

@esc     = \\ ( $escSimple | @escByte | @escUni4 | @escUni8 )
@char    = \' ( $bareScalar | @esc ) \'
@strChar = ( $strBare | $escSimple | @escByte | @escUni4 | @escUni8 )
@string  = \" @strChar* \"

miniml :-

  $tab                           { tok TokenKindTab }
  $whitespace+                   { tok TokenKindWhitespace }
  "--".*                         { tok TokenKindComment }
  $newline                       { tok TokenKindNewline }

  "("                            { tok TokenKindLParen }
  ")"                            { tok TokenKindRParen }
  "{"                            { tok TokenKindLBrace }
  "}"                            { tok TokenKindRBrace }
  "["                            { tok TokenKindLBracket }
  "]"                            { tok TokenKindRBracket }
  "!"                            { tok TokenKindBang }
  "#"                            { tok TokenKindHash }
  [\\]                           { tok TokenKindBackSlash }
  ":"                            { tok TokenKindColon }
  ";"                            { tok TokenKindSemi }
  ","                            { tok TokenKindComma }
  "."                            { tok TokenKindPeriod }
  "="                            { tok TokenKindEq }
  "<-"                           { tok TokenKindLArrow }
  "->"                           { tok TokenKindRArrow }
  "=>"                           { tok TokenKindLFatArrow }
  "|"                            { tok TokenKindBar }
  "_"                            { tok TokenKindUnderscore }

  @lowerCaseIdent                { tok TokenKindLowercaseIdent }
  @upperCaseIdent                { tok TokenKindUppercaseIdent }
  @conOpIdent                    { tok TokenKindConOpIdent }
  @opIdent                       { tok TokenKindOpIdent }

  @int                           { tok TokenKindInt }
  @char                          { tok TokenKindChar }
  @string                        { tok TokenKindString }
{

data LexerState = LexerState
  { lexerInput    :: !ByteString
  , lexerPos      :: !AlexPosn
  , lexerLastChar :: !Char
  , lexerErrors   :: ![Report Text]
  }

type Lexer a = State LexerState a

-- Helper to log an error into the state
logDiagnostic :: AlexPosn -> String -> Lexer ()
logDiagnostic (AlexPn _ line col) msg = modify $ \st ->
  let -- Construct your 'diagnose' report here 
      -- (Adjust this to match your exact 'diagnose' configuration/markers)
      newReport = Err Nothing ("Lexical Error: " ++ msg) [(Position line col line (col + 1) "src", Where msg)] []
  in st { lexerErrors = newReport : lexerErrors st }

tok :: TokenKind -> AlexPosn -> ByteString -> Token
tok kind p bs = Token kind (makeSpan p bs)

makeInt :: Int -> ByteString -> TokenKind
makeInt 10 bs = TokenKindInt $ parseRadix 10 $ bsToText bs
makeInt 2 bs = TokenKindInt $ parseRadix 2 $ stripIntPrefix bs
makeInt 8 bs = TokenKindInt $ parseRadix 8 $ stripIntPrefix bs
makeInt 16 bs = TokenKindInt $ parseRadix 16 $ stripIntPrefix bs
makeInt r _ = error $ "Unsupported radix" ++ show r

stripIntPrefix :: ByteString -> Text
stripIntPrefix bs = T.drop 2 $ bsToText bs

bsToText :: ByteString -> Text
bsToText = TE.decodeUtf8 . BL.toStrict

bToChar :: ByteString -> Char
bToChar = T.head . stripCharQuotes
  where
    stripCharQuotes :: ByteString -> Text
    stripCharQuotes = fromMaybe (error "Invalid char literal") . stripPrefix "'" . T.init . bsToText

parseRadix :: (Integral a) => a -> Text -> a
parseRadix r = T.foldl' step 0
  where
    step a c = a * r + (fromIntegral $ Char.digitToInt c)

makeSpan :: AlexPosn -> ByteString -> Span
makeSpan (AlexPn start _ _) bs = Span start end
  where 
    end = start + (fromIntegral $ BL.length bs)

posnOffset :: AlexPosn -> Int
posnOffset (AlexPn o _ _) = o

tokenize :: ByteString -> [Token]
tokenize = alexScanTokens
}