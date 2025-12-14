{
module Idyllic.Syn.Lexer (tokenize) where
import Idyllic.Syn.Token
import Data.ByteString.Lazy (ByteString)
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text.Encoding as TE
import Data.Text (Text, stripPrefix)
import qualified Data.Text as T
import qualified Data.Char as Char
import Idyllic.Utils.Span
import Data.Maybe (fromMaybe)
}

%wrapper "posn-bytestring"

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
$escSimple = [0\'\"\\nrtabfv] 

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

  $tab                           { \p bs -> Token TokenKindTab (makeSpan p bs) }
  $whitespace+                   { \p bs -> Token TokenKindWhitespace (makeSpan p bs) }
  "--".*                         ;
  $newline                       { \p bs -> Token TokenKindNewline (makeSpan p bs) }

  "("                            { \p bs -> Token TokenKindLParen (makeSpan p bs) }
  ")"                            { \p bs -> Token TokenKindRParen (makeSpan p bs) }
  "{"                            { \p bs -> Token TokenKindLBrace (makeSpan p bs) }
  "}"                            { \p bs -> Token TokenKindRBrace (makeSpan p bs) }
  "["                            { \p bs -> Token TokenKindLBracket (makeSpan p bs) }
  "]"                            { \p bs -> Token TokenKindRBracket (makeSpan p bs) }
  "!"                            { \p bs -> Token TokenKindBang (makeSpan p bs) }
  "#"                            { \p bs -> Token TokenKindHash (makeSpan p bs) }
  [\\]                           { \p bs -> Token TokenKindBackSlash (makeSpan p bs) }
  ":"                            { \p bs -> Token TokenKindColon (makeSpan p bs) }
  ";"                            { \p bs -> Token TokenKindSemi (makeSpan p bs) }
  ","                            { \p bs -> Token TokenKindComma (makeSpan p bs) }
  "."                            { \p bs -> Token TokenKindPeriod (makeSpan p bs) }
  "="                            { \p bs -> Token TokenKindEq (makeSpan p bs) }
  "<-"                           { \p bs -> Token TokenKindLArrow (makeSpan p bs) }
  "->"                           { \p bs -> Token TokenKindRArrow (makeSpan p bs) }
  "=>"                           { \p bs -> Token TokenKindLFatArrow (makeSpan p bs) }
  "|"                            { \p bs -> Token TokenKindBar (makeSpan p bs) }
  "_"                            { \p bs -> Token TokenKindUnderscore (makeSpan p bs) }

  @lowerCaseIdent                { \p bs -> Token TokenKindLowercaseIdent (makeSpan p bs) }
  @upperCaseIdent                { \p bs -> Token TokenKindUppercaseIdent (makeSpan p bs) }
  @conOpIdent                    { \p bs -> Token TokenKindConOpIdent (makeSpan p bs) }
  @opIdent                       { \p bs -> Token TokenKindOpIdent (makeSpan p bs) }

  @int                           { \p bs -> Token TokenKindInt (makeSpan p bs) }
  @char                          { \p bs -> Token TokenKindChar (makeSpan p bs) }
  @string                        { \p bs -> Token TokenKindString (makeSpan p bs) }

  $nonWhite                      { \p bs -> Token TokenKindError (makeSpan p bs) }

{
makeSpan :: AlexPosn -> ByteString -> Span
makeSpan (AlexPn start _ _) bs = Span start end
  where 
    end = start + (fromIntegral $ BL.length bs)

posnOffset :: AlexPosn -> Int
posnOffset (AlexPn o _ _) = o

tokenize :: ByteString -> [Token]
tokenize = alexScanTokens
}