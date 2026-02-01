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
import Idyllic.Utils.Located
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

  -- $tab                           { \p bs -> Located TokenTab (makeSpan p bs) }
  -- $whitespace+                   { \p bs -> Located TokenWhitespace (makeSpan p bs) }
  "--".*                         ;
  -- $newline                       { \p bs -> Located TokenNewline (makeSpan p bs) }
  $white+                        ;

  "("                            { \p bs -> Located TokenLParen (makeSpan p bs) }
  ")"                            { \p bs -> Located TokenRParen (makeSpan p bs) }
  "{"                            { \p bs -> Located TokenLBrace (makeSpan p bs) }
  "}"                            { \p bs -> Located TokenRBrace (makeSpan p bs) }
  "["                            { \p bs -> Located TokenLBracket (makeSpan p bs) }
  "]"                            { \p bs -> Located TokenRBracket (makeSpan p bs) }
  "!"                            { \p bs -> Located TokenBang (makeSpan p bs) }
  "#"                            { \p bs -> Located TokenHash (makeSpan p bs) }
  [\\]                           { \p bs -> Located TokenBackSlash (makeSpan p bs) }
  ":"                            { \p bs -> Located TokenColon (makeSpan p bs) }
  ";"                            { \p bs -> Located TokenSemi (makeSpan p bs) }
  ","                            { \p bs -> Located TokenComma (makeSpan p bs) }
  "."                            { \p bs -> Located TokenPeriod (makeSpan p bs) }
  "="                            { \p bs -> Located TokenEq (makeSpan p bs) }
  "<-"                           { \p bs -> Located TokenLArrow (makeSpan p bs) }
  "->"                           { \p bs -> Located TokenRArrow (makeSpan p bs) }
  "=>"                           { \p bs -> Located TokenLFatArrow (makeSpan p bs) }
  "|"                            { \p bs -> Located TokenBar (makeSpan p bs) }
  "_"                            { \p bs -> Located TokenUnderscore (makeSpan p bs) }

  @lowerCaseIdent                { \p bs -> Located TokenLowercaseIdent (makeSpan p bs) }
  @upperCaseIdent                { \p bs -> Located TokenUppercaseIdent (makeSpan p bs) }
  @conOpIdent                    { \p bs -> Located TokenConOpIdent (makeSpan p bs) }
  @opIdent                       { \p bs -> Located TokenOpIdent (makeSpan p bs) }

  @int                           { \p bs -> Located TokenInt (makeSpan p bs) }
  @char                          { \p bs -> Located TokenChar (makeSpan p bs) }
  @string                        { \p bs -> Located TokenString (makeSpan p bs) }

  $nonWhite                      { \p bs -> Located TokenError (makeSpan p bs) }

{
makeSpan :: AlexPosn -> ByteString -> Span
makeSpan (AlexPn start _ _) bs = Span start end
  where 
    end = start + (fromIntegral $ BL.length bs)

posnOffset :: AlexPosn -> Int
posnOffset (AlexPn o _ _) = o

tokenize :: ByteString -> [LToken]
tokenize = alexScanTokens
}