// A stream mode for Intel-syntax assembly (.asm / .nasm) — NASM, MASM and TASM.
//
// `@codemirror/legacy-modes` only ships GNU as, whose rules are wrong here in
// the two places you notice first: comments start with `;` rather than `#`, and
// operands read left-to-right with no `%` on registers. Rather than mislabel
// these files, this covers the syntax both assemblers share, with the union of
// their directive sets — the alternative is a mode per assembler for what is
// otherwise the same language.

// Instruction set: the integer core, plus the x87/SSE names common enough to
// meet in ordinary code. Not exhaustive — an unknown mnemonic falls through as
// a plain name, which is the right failure.
const MNEMONICS = new Set(
  `aaa aad aam aas adc add and bsf bsr bswap bt btc btr bts call cbw cdq cdqe
   clc cld cli clts cmc cmp cmpsb cmpsw cmpsd cmpsq cmpxchg cmpxchg8b cpuid cqo
   cwd cwde daa das dec div enter hlt idiv imul in inc ins int int3 into invlpg
   iret iretd ja jae jb jbe jc jcxz je jecxz jg jge jl jle jmp jna jnae jnb jnbe
   jnc jne jng jnge jnl jnle jno jnp jns jnz jo jp jpe jpo jrcxz js jz lahf lar
   lds lea leave les lfence lfs lgs lgdt lidt lldt lmsw lock lodsb lodsd lodsq
   lodsw loop loope loopne loopnz loopz lsl lss ltr mfence mov movbe movsb movsd
   movsq movsw movsx movsxd movzx mul neg nop not or out outs pause pop popa
   popad popcnt popf popfd popfq push pusha pushad pushf pushfd pushfq rcl rcr
   rdmsr rdpmc rdtsc rdtscp rep repe repne repnz repz ret retf retn rol ror rsm
   sahf sal sar sbb scasb scasd scasq scasw seta setae setb setbe setc sete setg
   setge setl setle setna setnb setnc setne setng setnl setno setnp setns setnz
   seto setp setpo sets setz sfence sgdt shl shld shr shrd sidt sldt smsw stc
   std sti stosb stosd stosq stosw str sub swapgs syscall sysenter sysexit
   sysret test ud2 verr verw wait wbinvd wrmsr xadd xchg xgetbv xlat xlatb xor
   addps addsd addss andps comisd comiss cvtsi2sd cvtsi2ss cvtsd2ss cvtss2sd
   cvttsd2si cvttss2si divps divsd divss emms fabs fadd faddp fchs fcom fcomp
   fcompp fdiv fdivp fdivr fild finit fistp fld fld1 fldz fmul fmulp fnstsw
   fstp fsub fsubp fsubr ftst fucom fucomp fwait fxch ldmxcsr maxsd maxss minsd
   minss movaps movd movdqa movdqu movhps movlps movq movsd movss movups mulps
   mulsd mulss orps pand pcmpeqb pmovmskb por prefetch pshufd pslld psrld
   psubb punpcklbw pxor rcpps rsqrtps shufps sqrtsd sqrtss stmxcsr subps subsd
   subss ucomisd ucomiss unpcklps vaddpd vaddps vmovaps vmovdqa vmovdqu vmovups
   vpxor vxorps xorps`.split(/\s+/),
);

// Pseudo-ops and directives. NASM and MASM overlap only partly, so both sets
// are here; MASM's dotted forms (.code, .data) are matched with the dot.
const DIRECTIVES = new Set(
  `bits use16 use32 use64 section segment ends group absolute extern extrn
   global public common import export org align alignb even default cpu float
   db dw dd dq dt ddq do dy dz resb resw resd resq rest resdq reso resy resz
   incbin equ times struc endstruc istruc at iend union record proc endp end
   macro endm purge label textequ catstr substr instr sizestr option assume
   includelib include comment title subtitle page name model stack const code
   data data? fardata fardata? if1 if2 else endif locals nolocals list nolist
   byte sbyte word sword dword sdword fword qword tbyte real4 real8 real10
   invoke uses local echo err .386 .486 .586 .686 .mmx .xmm .k3d .8086 .8087
   .186 .286 .287 .387 .radix .listall .startup .exit .model .code .data .const
   .stack .fardata .dosseg .seq .alpha .text .bss .rodata .idata`.split(/\s+/),
);

// Operand decorations rather than instructions: `mov dword ptr [x], 1`. These
// are styled as modifiers, and the directives above as types, because the
// editor's palette leaves `builtin` the same colour as an ordinary name — a
// directive has to stand out from the label sitting next to it.
const OPERAND_KEYWORDS = new Set(
  `ptr offset seg short near far type sizeof lengthof low high lowword highword
   dup this addr abs strict nosplit rel wrt`.split(/\s+/),
);

const REGISTER =
  /^(?:[re]?(?:ax|bx|cx|dx|si|di|bp|sp|ip)|[abcd][hl]|[sb]pl|[sd]il|r(?:8|9|1[0-5])[bwd]?|[cdefgs]s|[cd]r[0-7]|tr[3-7]|st[0-7]|st\(\d\)|[xyz]mm(?:3[01]|[12]?\d)|mm[0-7]|k[0-7])\b/i;

// Numbers, in every spelling the two assemblers accept: C-style prefixes, the
// assembler's own `0` prefixes, and the trailing radix letters (1Fh, 1010b).
// The prefixed forms are tried first so `0x1Fh` cannot be split. There is no
// leading-dot form: both assemblers want a digit before the point, so `.686`
// is the CPU directive and never a fraction.
const NUMBER =
  /^(?:0[xXhH][\da-fA-F_]+|\$[\da-fA-F_]+|0[bByY][01_]+|0[oOqQ][0-7_]+|0[dDtT]\d+|\d[\d_]*\.\d*(?:[eE][+-]?\d+)?|[\da-fA-F][\da-fA-F_]*[hH]|\d[\d_]*[bByYoOqQdDtT]?(?:[eE][+-]?\d+)?)/;

/** @type {import("@codemirror/language").StreamParser<{comment: boolean}>} */
export const intelAsm = {
  name: "intelasm",

  // MASM's block comment is `COMMENT <delim> ... <delim>`, which no editor can
  // parse without knowing the delimiter; `comment` only tracks NASM's `%comment`.
  startState() {
    return { comment: false };
  },

  token(stream, state) {
    if (state.comment) {
      if (stream.match(/^%endcomment\b/i)) {
        state.comment = false;
        return "comment";
      }
      stream.skipToEnd();
      return "comment";
    }

    if (stream.eatSpace()) return null;

    if (stream.eat(";")) {
      stream.skipToEnd();
      return "comment";
    }

    // NASM's preprocessor. `%%label` and `%1` inside macros are arguments, not
    // directives, but they are still preprocessor territory.
    if (stream.peek() === "%") {
      if (stream.match(/^%comment\b/i)) {
        state.comment = true;
        return "comment";
      }
      stream.next();
      stream.match(/^%?[\w$.]*/);
      return "meta";
    }

    if (stream.match(/^(?:"[^"]*"?|'[^']*'?|`[^`]*`?)/)) return "string";

    // `$` is the current address and `$$` the section start; a `$` glued to a
    // name is part of that name (NASM escapes reserved words that way).
    if (stream.match(/^\$\$?(?![\w$])/)) return "atom";

    if (stream.match(NUMBER)) return "number";

    if (stream.match(REGISTER)) return "variableName.special";

    // A label: `name:`, NASM's local `.loop:`, or a leading `.` label at the
    // start of a line. The colon is consumed with it.
    if (stream.match(/^[.@?$\w]+:/)) return "labelName";

    if (stream.match(/^[.@?$\w]+/)) {
      const word = stream.current().toLowerCase();
      if (MNEMONICS.has(word)) return "keyword";
      if (DIRECTIVES.has(word)) return "typeName";
      if (OPERAND_KEYWORDS.has(word)) return "modifier";
      return "variableName";
    }

    if (stream.match(/^(?:<<|>>|\|\||&&|[-+*/%&|^~!<>=]|\.\.)/)) return "operator";
    if (stream.match(/^[[\]()]/)) return "bracket";
    if (stream.match(/^[,:]/)) return "punctuation";

    stream.next();
    return null;
  },

  languageData: {
    commentTokens: { line: ";" },
  },
};
