/**
 * The CSS the engine accepts, as data: the properties, the values
 * each one takes, the selectors the vocabulary names, and the
 * at-rules that parse.
 *
 * A host with a style editor completes from this. It needs no book
 * laid out and no file beside the package, and it describes the
 * engine in the module it is read from rather than the engine of
 * some other release.
 *
 * The `syntax` strings are written in CSS value-definition syntax. A
 * bare word is a keyword, `<name>` is a type, and `name()` is a
 * function. The keywords are listed on their own as well, for a host
 * that completes them rather than reads the grammar.
 *
 * This file is generated from the parser's own tables, and it holds
 * the description that `fleuron --css-subset` writes. Do not edit it.
 * Run `FLEURON_UPDATE_DOCS=1 cargo test -p fleuron --test css_subset`.
 */

/** What the engine accepts, from the version that produced it. */
export interface Subset {
  /** The engine version this description came from. */
  version: string;
  /** What a style rule selects. */
  selectors: Selectors;
  /** The shape of one declaration, in value-definition syntax. */
  declaration: string;
  /**
   * The shape of one custom property declaration, in
   * value-definition syntax.
   */
  custom_property: string;
  /**
   * The function that puts a custom property's value in a
   * declaration, in value-definition syntax.
   */
  var: string;
  /** The properties a style rule declares. */
  properties: Property[];
  /** The `@page` rule. */
  page: PageRule;
  /** The `@font-face` rule. */
  font_face: FontFaceRule;
  /** The units a `<length>` carries. */
  units: string[];
  /** The names a `<color>` takes. */
  color_names: string[];
}

/** The selector vocabulary. */
export interface Selectors {
  /** The element names, as the content tree produces them. */
  elements: string[];
  /** What a compound is made of besides pseudo-classes. */
  compounds: Selector[];
  /** The combinators between two compounds. */
  combinators: Selector[];
  /** How selectors join in a list. */
  list: Selector;
  /** The pseudo-classes, functional ones with their parentheses. */
  pseudo_classes: Selector[];
  /** The pseudo-elements. */
  pseudo_elements: Selector[];
  /**
   * The properties `::first-line` takes, out of the properties a
   * style rule declares.
   */
  first_line_properties: string[];
}

/** One piece of selector syntax, with a selector that uses it. */
export interface Selector {
  /** The syntax as it is written. */
  name: string;
  /** A whole selector that parses. */
  example: string;
}

/** One property. */
export interface Property {
  /** The property name. */
  name: string;
  /** Whether a child starts from the parent's value. */
  inherited: boolean;
  /** The values it accepts, in CSS value-definition syntax. */
  syntax: string;
  /**
   * The keywords in `syntax`, wherever they stand in it. One that
   * only follows another value, like `landscape` after a page size,
   * is not a value on its own.
   */
  keywords: string[];
  /** Values that parse. */
  examples: string[];
}

/** One `@font-face` descriptor. */
export interface Descriptor {
  /** The descriptor name. */
  name: string;
  /** The values it accepts, in CSS value-definition syntax. */
  syntax: string;
  /** The keywords in `syntax`, wherever they stand in it. */
  keywords: string[];
  /** Values that parse. */
  examples: string[];
}

/** The `@page` rule. */
export interface PageRule {
  /**
   * What follows `@page`, in value-definition syntax: a page name,
   * then any of the page selectors.
   */
  prelude: string;
  /** The page selectors, without their colon. */
  selectors: string[];
  /** The properties a page body declares. */
  properties: Property[];
  /** The margin boxes a page body opens. */
  margin_boxes: MarginBoxDescription[];
  /**
   * The properties a margin box declares on top of the style
   * properties, which it also accepts.
   */
  margin_box_properties: Property[];
  /** The named sheets `size` accepts, portrait. */
  sizes: PageSize[];
  /** The counter styles `counter(page, ...)` accepts. */
  counter_styles: string[];
}

/** One margin box. */
export interface MarginBoxDescription {
  /** The at-rule name, without the `@`. */
  name: string;
  /**
   * Whether the engine draws it. A box it does not draw is read and
   * dropped.
   */
  paints: boolean;
}

/** One named page size. */
export interface PageSize {
  /** The keyword. */
  name: string;
  /** Width in points, portrait. */
  width: number;
  /** Height in points, portrait. */
  height: number;
}

/** The `@font-face` rule. */
export interface FontFaceRule {
  /** The descriptors a face body declares. */
  descriptors: Descriptor[];
}


export const SUBSET: Subset = {
  version: '0.15.0',
  selectors: {
    elements: [
      'book',
      'section',
      'notes',
      'note',
      'h1',
      'h2',
      'h3',
      'h4',
      'h5',
      'h6',
      'p',
      'blockquote',
      'pre',
      'hr',
      'img',
      'ul',
      'ol',
      'li',
      'table',
      'thead',
      'tbody',
      'tr',
      'th',
      'td',
      'code',
      'em',
      'strong',
      'a',
      's',
      'span'
    ],
    compounds: [
      {
        name: '<element>',
        example: 'p'
      },
      {
        name: '*',
        example: 'section > *'
      },
      {
        name: '.<class>',
        example: 'p.epigraph'
      },
      {
        name: '#<id>',
        example: '#frontispiece'
      }
    ],
    combinators: [
      {
        name: 'descendant',
        example: 'section p'
      },
      {
        name: 'child',
        example: 'section > p'
      },
      {
        name: 'next-sibling',
        example: 'h1 + p'
      },
      {
        name: 'subsequent-sibling',
        example: 'h1 ~ p'
      }
    ],
    list: {
      name: ',',
      example: 'h1, h2'
    },
    pseudo_classes: [
      {
        name: ':first-child',
        example: 'p:first-child'
      },
      {
        name: ':last-child',
        example: 'p:last-child'
      },
      {
        name: ':only-child',
        example: 'p:only-child'
      },
      {
        name: ':nth-child()',
        example: 'p:nth-child(2n+1)'
      },
      {
        name: ':nth-last-child()',
        example: 'p:nth-last-child(2)'
      },
      {
        name: ':first-of-type',
        example: 'p:first-of-type'
      },
      {
        name: ':last-of-type',
        example: 'p:last-of-type'
      },
      {
        name: ':only-of-type',
        example: 'p:only-of-type'
      },
      {
        name: ':nth-of-type()',
        example: 'p:nth-of-type(2)'
      },
      {
        name: ':nth-last-of-type()',
        example: 'p:nth-last-of-type(2)'
      },
      {
        name: ':empty',
        example: 'p:empty'
      },
      {
        name: ':root',
        example: ':root'
      },
      {
        name: ':is()',
        example: ':is(h1, h2)'
      },
      {
        name: ':where()',
        example: ':where(h1, h2)'
      },
      {
        name: ':not()',
        example: 'p:not(:first-child)'
      },
      {
        name: ':has()',
        example: 'p:has(em)'
      }
    ],
    pseudo_elements: [
      {
        name: '::first-letter',
        example: 'p::first-letter'
      },
      {
        name: '::first-line',
        example: 'p::first-line'
      },
      {
        name: '::before',
        example: 'a::before'
      },
      {
        name: '::after',
        example: 'a::after'
      }
    ],
    first_line_properties: [
      'color',
      'font-family',
      'font-size',
      'font-style',
      'font-weight',
      'font-variant-caps',
      'letter-spacing',
      'text-transform',
      'text-decoration-line',
      'text-decoration-color',
      'text-decoration-style',
      'text-decoration-thickness',
      'text-decoration'
    ]
  },
  declaration: '<property>: <value> !important?',
  custom_property: '--<name>: <value> !important?',
  var: 'var( --<name> [, <fallback> ]? )',
  properties: [
    {
      name: 'font-family',
      inherited: true,
      syntax: '[ <family-name> | serif | sans-serif | monospace ]#',
      keywords: [
        'serif',
        'sans-serif',
        'monospace'
      ],
      examples: [
        '"Author Serif", serif',
        'Times New Roman'
      ]
    },
    {
      name: 'font-size',
      inherited: true,
      syntax: '<length> | <percentage>',
      keywords: [],
      examples: [
        '11pt',
        '1.5em',
        '120%'
      ]
    },
    {
      name: 'font-style',
      inherited: true,
      syntax: 'normal | italic | oblique',
      keywords: [
        'normal',
        'italic',
        'oblique'
      ],
      examples: [
        'italic'
      ]
    },
    {
      name: 'font-weight',
      inherited: true,
      syntax: 'normal | bold | <number [1,1000]>',
      keywords: [
        'normal',
        'bold'
      ],
      examples: [
        'bold',
        '600'
      ]
    },
    {
      name: 'color',
      inherited: true,
      syntax: '<color>',
      keywords: [],
      examples: [
        'darkslategray',
        '#369',
        '#336699',
        '#3698',
        '#33669980',
        'rgb(51, 102, 153)',
        'rgb(20% 40% 60%)',
        'rgb(51 102 153 / 50%)',
        'rgba(51, 102, 153, 0.5)'
      ]
    },
    {
      name: 'line-height',
      inherited: true,
      syntax: 'normal | <number> | <length> | <percentage>',
      keywords: [
        'normal'
      ],
      examples: [
        'normal',
        '1.4',
        '14pt',
        '140%'
      ]
    },
    {
      name: 'letter-spacing',
      inherited: true,
      syntax: 'normal | <length>',
      keywords: [
        'normal'
      ],
      examples: [
        'normal',
        '0.05em'
      ]
    },
    {
      name: 'font-variant-caps',
      inherited: true,
      syntax: 'normal | small-caps',
      keywords: [
        'normal',
        'small-caps'
      ],
      examples: [
        'small-caps'
      ]
    },
    {
      name: 'font-feature-settings',
      inherited: true,
      syntax: 'normal | <feature-tag-value>#',
      keywords: [
        'normal'
      ],
      examples: [
        '"ss01" 1',
        '"liga" 0',
        '"onum"',
        'normal'
      ]
    },
    {
      name: 'font-variant-ligatures',
      inherited: true,
      syntax: 'normal | none | [ common-ligatures | no-common-ligatures ] || [ discretionary-ligatures | no-discretionary-ligatures ] || [ historical-ligatures | no-historical-ligatures ] || [ contextual | no-contextual ]',
      keywords: [
        'normal',
        'none',
        'common-ligatures',
        'no-common-ligatures',
        'discretionary-ligatures',
        'no-discretionary-ligatures',
        'historical-ligatures',
        'no-historical-ligatures',
        'contextual',
        'no-contextual'
      ],
      examples: [
        'discretionary-ligatures',
        'no-common-ligatures contextual'
      ]
    },
    {
      name: 'font-variant-numeric',
      inherited: true,
      syntax: 'normal | [ lining-nums | oldstyle-nums ] || [ proportional-nums | tabular-nums ] || [ diagonal-fractions | stacked-fractions ] || ordinal || slashed-zero',
      keywords: [
        'normal',
        'lining-nums',
        'oldstyle-nums',
        'proportional-nums',
        'tabular-nums',
        'diagonal-fractions',
        'stacked-fractions',
        'ordinal',
        'slashed-zero'
      ],
      examples: [
        'oldstyle-nums',
        'lining-nums tabular-nums'
      ]
    },
    {
      name: 'font-variant-alternates',
      inherited: true,
      syntax: 'normal | historical-forms',
      keywords: [
        'normal',
        'historical-forms'
      ],
      examples: [
        'historical-forms'
      ]
    },
    {
      name: 'text-transform',
      inherited: true,
      syntax: 'none | uppercase | lowercase | capitalize',
      keywords: [
        'none',
        'uppercase',
        'lowercase',
        'capitalize'
      ],
      examples: [
        'uppercase'
      ]
    },
    {
      name: 'text-decoration-line',
      inherited: true,
      syntax: 'none | [ underline || overline || line-through ]',
      keywords: [
        'none',
        'underline',
        'overline',
        'line-through'
      ],
      examples: [
        'underline',
        'line-through',
        'underline overline'
      ]
    },
    {
      name: 'text-decoration-color',
      inherited: true,
      syntax: 'currentcolor | <color>',
      keywords: [
        'currentcolor'
      ],
      examples: [
        '#808080',
        'currentcolor'
      ]
    },
    {
      name: 'text-decoration-style',
      inherited: true,
      syntax: 'solid | double',
      keywords: [
        'solid',
        'double'
      ],
      examples: [
        'double'
      ]
    },
    {
      name: 'text-decoration-thickness',
      inherited: true,
      syntax: 'auto | from-font | <length>',
      keywords: [
        'auto',
        'from-font'
      ],
      examples: [
        '0.06em',
        'auto',
        'from-font'
      ]
    },
    {
      name: 'text-decoration',
      inherited: true,
      syntax: 'none | [ underline || overline || line-through ] || solid || double || <color> || <length>',
      keywords: [
        'none',
        'underline',
        'overline',
        'line-through',
        'solid',
        'double'
      ],
      examples: [
        'underline',
        'line-through',
        'underline #808080',
        'underline double 1pt'
      ]
    },
    {
      name: 'text-align',
      inherited: true,
      syntax: 'left | right | center | justify | start | end',
      keywords: [
        'left',
        'right',
        'center',
        'justify',
        'start',
        'end'
      ],
      examples: [
        'justify'
      ]
    },
    {
      name: 'text-justify',
      inherited: true,
      syntax: 'auto | inter-word | inter-character | distribute',
      keywords: [
        'auto',
        'inter-word',
        'inter-character',
        'distribute'
      ],
      examples: [
        'inter-character'
      ]
    },
    {
      name: 'text-indent',
      inherited: true,
      syntax: '<length> | <percentage>',
      keywords: [],
      examples: [
        '1.2em',
        '5%'
      ]
    },
    {
      name: 'hanging-punctuation',
      inherited: true,
      syntax: 'none | [ first || [ force-end | allow-end ] || last ]',
      keywords: [
        'none',
        'first',
        'force-end',
        'allow-end',
        'last'
      ],
      examples: [
        'none',
        'first',
        'first allow-end last',
        'force-end'
      ]
    },
    {
      name: 'hyphens',
      inherited: true,
      syntax: 'none | manual | auto',
      keywords: [
        'none',
        'manual',
        'auto'
      ],
      examples: [
        'auto'
      ]
    },
    {
      name: 'orphans',
      inherited: true,
      syntax: '<integer>',
      keywords: [],
      examples: [
        '3'
      ]
    },
    {
      name: 'widows',
      inherited: true,
      syntax: '<integer>',
      keywords: [],
      examples: [
        '3'
      ]
    },
    {
      name: 'page',
      inherited: true,
      syntax: 'auto | <name>',
      keywords: [
        'auto'
      ],
      examples: [
        'auto',
        'chapter'
      ]
    },
    {
      name: 'border-collapse',
      inherited: true,
      syntax: 'separate | collapse',
      keywords: [
        'separate',
        'collapse'
      ],
      examples: [
        'collapse',
        'separate'
      ]
    },
    {
      name: 'list-style-type',
      inherited: true,
      syntax: 'disc | circle | square | decimal | lower-roman | upper-roman | lower-alpha | upper-alpha | none',
      keywords: [
        'disc',
        'circle',
        'square',
        'decimal',
        'lower-roman',
        'upper-roman',
        'lower-alpha',
        'upper-alpha',
        'none'
      ],
      examples: [
        'decimal',
        'none'
      ]
    },
    {
      name: 'content',
      inherited: false,
      syntax: 'none | [ <string> | target-counter( <target> , page , <counter-style>? ) | target-text( <target> ) ]+',
      keywords: [
        'none'
      ],
      examples: [
        'none',
        '"\\2766"',
        '" (page " target-counter(attr(href url), page) ")"',
        'target-counter("#the-hunter", page, upper-roman)',
        'target-text(attr(href url))'
      ]
    },
    {
      name: 'string-set',
      inherited: false,
      syntax: 'none | [ <name> [ content() | content(text) | <string> ]+ ]#',
      keywords: [
        'none'
      ],
      examples: [
        'none',
        'chapter content()',
        'part "Part " content(), chapter content()'
      ]
    },
    {
      name: 'counter-reset',
      inherited: false,
      syntax: 'none | [ page <integer>? || note <integer>? ]',
      keywords: [
        'none',
        'page',
        'note'
      ],
      examples: [
        'none',
        'page',
        'page 1',
        'note',
        'note 1'
      ]
    },
    {
      name: 'initial-letter',
      inherited: false,
      syntax: '<integer>',
      keywords: [],
      examples: [
        '3'
      ]
    },
    {
      name: 'position',
      inherited: false,
      syntax: 'static | relative | absolute',
      keywords: [
        'static',
        'relative',
        'absolute'
      ],
      examples: [
        'relative',
        'absolute'
      ]
    },
    {
      name: 'top',
      inherited: false,
      syntax: 'auto | <length> | <percentage>',
      keywords: [
        'auto'
      ],
      examples: [
        'auto',
        '0',
        '-12pt',
        '10%'
      ]
    },
    {
      name: 'right',
      inherited: false,
      syntax: 'auto | <length> | <percentage>',
      keywords: [
        'auto'
      ],
      examples: [
        'auto',
        '0',
        '-12pt',
        '10%'
      ]
    },
    {
      name: 'bottom',
      inherited: false,
      syntax: 'auto | <length> | <percentage>',
      keywords: [
        'auto'
      ],
      examples: [
        'auto',
        '0',
        '-12pt',
        '10%'
      ]
    },
    {
      name: 'left',
      inherited: false,
      syntax: 'auto | <length> | <percentage>',
      keywords: [
        'auto'
      ],
      examples: [
        'auto',
        '0',
        '-12pt',
        '10%'
      ]
    },
    {
      name: 'z-index',
      inherited: false,
      syntax: 'auto | <integer>',
      keywords: [
        'auto'
      ],
      examples: [
        'auto',
        '10',
        '-1'
      ]
    },
    {
      name: 'opacity',
      inherited: false,
      syntax: '<number> | <percentage>',
      keywords: [],
      examples: [
        '0.05',
        '50%'
      ]
    },
    {
      name: 'wrap-flow',
      inherited: false,
      syntax: 'auto | both | start | end',
      keywords: [
        'auto',
        'both',
        'start',
        'end'
      ],
      examples: [
        'end'
      ]
    },
    {
      name: 'shape-outside',
      inherited: false,
      syntax: 'none | auto | polygon( [ <length> | <percentage> ]{2} [ , [ <length> | <percentage> ]{2} ]* )',
      keywords: [
        'none',
        'auto'
      ],
      examples: [
        'auto',
        'polygon(0 0, 100% 0, 100% 100%)'
      ]
    },
    {
      name: 'shape-margin',
      inherited: false,
      syntax: '<length> | <percentage>',
      keywords: [],
      examples: [
        '6pt'
      ]
    },
    {
      name: 'width',
      inherited: false,
      syntax: 'auto | <length> | <percentage>',
      keywords: [
        'auto'
      ],
      examples: [
        'auto',
        '8em',
        '25%'
      ]
    },
    {
      name: 'height',
      inherited: false,
      syntax: 'auto | <length> | <percentage>',
      keywords: [
        'auto'
      ],
      examples: [
        'auto',
        '3in',
        '50%'
      ]
    },
    {
      name: 'min-height',
      inherited: false,
      syntax: 'auto | <length> | <percentage>',
      keywords: [
        'auto'
      ],
      examples: [
        'auto',
        '2in',
        '25%'
      ]
    },
    {
      name: 'max-width',
      inherited: false,
      syntax: 'none | <length> | <percentage>',
      keywords: [
        'none'
      ],
      examples: [
        'none',
        '4in',
        '50%'
      ]
    },
    {
      name: 'max-height',
      inherited: false,
      syntax: 'none | <length> | <percentage>',
      keywords: [
        'none'
      ],
      examples: [
        'none',
        '3in',
        '40%'
      ]
    },
    {
      name: 'margin',
      inherited: false,
      syntax: '[ <length> | <percentage> ]{1,4}',
      keywords: [],
      examples: [
        '1em',
        '1em 2em',
        '1em 2em 0',
        '54pt 42pt 54pt 54pt'
      ]
    },
    {
      name: 'margin-top',
      inherited: false,
      syntax: '<length> | <percentage>',
      keywords: [],
      examples: [
        '1em'
      ]
    },
    {
      name: 'margin-right',
      inherited: false,
      syntax: '<length> | <percentage>',
      keywords: [],
      examples: [
        '1em'
      ]
    },
    {
      name: 'margin-bottom',
      inherited: false,
      syntax: '<length> | <percentage>',
      keywords: [],
      examples: [
        '1em'
      ]
    },
    {
      name: 'margin-left',
      inherited: false,
      syntax: '<length> | <percentage>',
      keywords: [],
      examples: [
        '1em'
      ]
    },
    {
      name: 'padding',
      inherited: false,
      syntax: '[ <length> | <percentage> ]{1,4}',
      keywords: [],
      examples: [
        '12pt',
        '6pt 12pt',
        '6pt 12pt 0',
        '6pt 12pt 6pt 12pt'
      ]
    },
    {
      name: 'padding-top',
      inherited: false,
      syntax: '<length> | <percentage>',
      keywords: [],
      examples: [
        '6pt'
      ]
    },
    {
      name: 'padding-right',
      inherited: false,
      syntax: '<length> | <percentage>',
      keywords: [],
      examples: [
        '6pt'
      ]
    },
    {
      name: 'padding-bottom',
      inherited: false,
      syntax: '<length> | <percentage>',
      keywords: [],
      examples: [
        '6pt'
      ]
    },
    {
      name: 'padding-left',
      inherited: false,
      syntax: '<length> | <percentage>',
      keywords: [],
      examples: [
        '6pt'
      ]
    },
    {
      name: 'border',
      inherited: false,
      syntax: '[ <length> | thin | medium | thick ] || [ none | solid ] || <color>',
      keywords: [
        'thin',
        'medium',
        'thick',
        'none',
        'solid'
      ],
      examples: [
        '2pt solid',
        'thin solid crimson',
        'none'
      ]
    },
    {
      name: 'border-top',
      inherited: false,
      syntax: '[ <length> | thin | medium | thick ] || [ none | solid ] || <color>',
      keywords: [
        'thin',
        'medium',
        'thick',
        'none',
        'solid'
      ],
      examples: [
        '2pt solid',
        'thin solid crimson',
        'none'
      ]
    },
    {
      name: 'border-right',
      inherited: false,
      syntax: '[ <length> | thin | medium | thick ] || [ none | solid ] || <color>',
      keywords: [
        'thin',
        'medium',
        'thick',
        'none',
        'solid'
      ],
      examples: [
        '2pt solid',
        'thin solid crimson',
        'none'
      ]
    },
    {
      name: 'border-bottom',
      inherited: false,
      syntax: '[ <length> | thin | medium | thick ] || [ none | solid ] || <color>',
      keywords: [
        'thin',
        'medium',
        'thick',
        'none',
        'solid'
      ],
      examples: [
        '2pt solid',
        'thin solid crimson',
        'none'
      ]
    },
    {
      name: 'border-left',
      inherited: false,
      syntax: '[ <length> | thin | medium | thick ] || [ none | solid ] || <color>',
      keywords: [
        'thin',
        'medium',
        'thick',
        'none',
        'solid'
      ],
      examples: [
        '2pt solid',
        'thin solid crimson',
        'none'
      ]
    },
    {
      name: 'border-width',
      inherited: false,
      syntax: '[ <length> | thin | medium | thick ]{1,4}',
      keywords: [
        'thin',
        'medium',
        'thick'
      ],
      examples: [
        '2pt',
        'thin thick',
        '1pt 2pt 1pt 2pt'
      ]
    },
    {
      name: 'border-style',
      inherited: false,
      syntax: '[ none | solid ]{1,4}',
      keywords: [
        'none',
        'solid'
      ],
      examples: [
        'solid',
        'none solid'
      ]
    },
    {
      name: 'border-color',
      inherited: false,
      syntax: '<color>{1,4}',
      keywords: [],
      examples: [
        'crimson',
        '#369 black'
      ]
    },
    {
      name: 'border-radius',
      inherited: false,
      syntax: '[ <length> | <percentage> ]{1,4} [ / [ <length> | <percentage> ]{1,4} ]?',
      keywords: [],
      examples: [
        '3pt',
        '3pt 6pt',
        '3pt 6pt 0',
        '3pt 6pt 0 1em',
        '50%',
        '12pt / 6pt'
      ]
    },
    {
      name: 'border-top-left-radius',
      inherited: false,
      syntax: '[ <length> | <percentage> ]{1,2}',
      keywords: [],
      examples: [
        '3pt',
        '6pt 3pt'
      ]
    },
    {
      name: 'border-top-right-radius',
      inherited: false,
      syntax: '[ <length> | <percentage> ]{1,2}',
      keywords: [],
      examples: [
        '3pt',
        '6pt 3pt'
      ]
    },
    {
      name: 'border-bottom-right-radius',
      inherited: false,
      syntax: '[ <length> | <percentage> ]{1,2}',
      keywords: [],
      examples: [
        '3pt',
        '6pt 3pt'
      ]
    },
    {
      name: 'border-bottom-left-radius',
      inherited: false,
      syntax: '[ <length> | <percentage> ]{1,2}',
      keywords: [],
      examples: [
        '3pt',
        '6pt 3pt'
      ]
    },
    {
      name: 'background-color',
      inherited: false,
      syntax: '<color> | transparent',
      keywords: [
        'transparent'
      ],
      examples: [
        '#f4f1ea',
        'transparent'
      ]
    },
    {
      name: 'background-image',
      inherited: false,
      syntax: 'none | <url>',
      keywords: [
        'none'
      ],
      examples: [
        'none',
        'url("scan.webp")',
        'url(art/plate.png)'
      ]
    },
    {
      name: 'background-repeat',
      inherited: false,
      syntax: 'repeat | no-repeat',
      keywords: [
        'repeat',
        'no-repeat'
      ],
      examples: [
        'repeat',
        'no-repeat'
      ]
    },
    {
      name: 'background-size',
      inherited: false,
      syntax: 'auto | cover | contain | [ <length> | <percentage> | auto ]{1,2}',
      keywords: [
        'auto',
        'cover',
        'contain'
      ],
      examples: [
        'auto',
        'cover',
        'contain',
        '120pt',
        '100% auto'
      ]
    },
    {
      name: 'background-position',
      inherited: false,
      syntax: '[ left | center | right | <length> | <percentage> ] [ top | center | bottom | <length> | <percentage> ]?',
      keywords: [
        'left',
        'center',
        'right',
        'top',
        'bottom'
      ],
      examples: [
        'center',
        'right bottom',
        '50% 50%',
        '12pt 18pt',
        'top left'
      ]
    },
    {
      name: 'box-decoration-break',
      inherited: false,
      syntax: 'slice | clone',
      keywords: [
        'slice',
        'clone'
      ],
      examples: [
        'slice',
        'clone'
      ]
    },
    {
      name: 'break-before',
      inherited: false,
      syntax: 'auto | avoid | avoid-page | avoid-column | column | page | always | left | right | recto | verso',
      keywords: [
        'auto',
        'avoid',
        'avoid-page',
        'avoid-column',
        'column',
        'page',
        'always',
        'left',
        'right',
        'recto',
        'verso'
      ],
      examples: [
        'recto'
      ]
    },
    {
      name: 'break-after',
      inherited: false,
      syntax: 'auto | avoid | avoid-page | avoid-column | column | page | always | left | right | recto | verso',
      keywords: [
        'auto',
        'avoid',
        'avoid-page',
        'avoid-column',
        'column',
        'page',
        'always',
        'left',
        'right',
        'recto',
        'verso'
      ],
      examples: [
        'avoid'
      ]
    },
    {
      name: 'break-inside',
      inherited: false,
      syntax: 'auto | avoid | avoid-page | avoid-column | column | page | always | left | right | recto | verso',
      keywords: [
        'auto',
        'avoid',
        'avoid-page',
        'avoid-column',
        'column',
        'page',
        'always',
        'left',
        'right',
        'recto',
        'verso'
      ],
      examples: [
        'avoid'
      ]
    },
    {
      name: 'column-span',
      inherited: false,
      syntax: 'none | all',
      keywords: [
        'none',
        'all'
      ],
      examples: [
        'all'
      ]
    }
  ],
  page: {
    prelude: '<name>? [ :first | :blank | :left | :right ]*',
    selectors: [
      'first',
      'blank',
      'left',
      'right'
    ],
    properties: [
      {
        name: 'size',
        inherited: false,
        syntax: '<length>{1,2} | <page-size> [ portrait | landscape ]?',
        keywords: [
          'portrait',
          'landscape'
        ],
        examples: [
          '432pt 648pt',
          '148mm',
          'a5',
          'letter landscape',
          'b5 portrait'
        ]
      },
      {
        name: 'margin',
        inherited: false,
        syntax: '[ <length> | <percentage> ]{1,4}',
        keywords: [],
        examples: [
          '54pt',
          '54pt 42pt',
          '54pt 42pt 54pt 54pt'
        ]
      },
      {
        name: 'margin-top',
        inherited: false,
        syntax: '<length> | <percentage>',
        keywords: [],
        examples: [
          '54pt'
        ]
      },
      {
        name: 'margin-right',
        inherited: false,
        syntax: '<length> | <percentage>',
        keywords: [],
        examples: [
          '42pt'
        ]
      },
      {
        name: 'margin-bottom',
        inherited: false,
        syntax: '<length> | <percentage>',
        keywords: [],
        examples: [
          '54pt'
        ]
      },
      {
        name: 'margin-left',
        inherited: false,
        syntax: '<length> | <percentage>',
        keywords: [],
        examples: [
          '54pt'
        ]
      },
      {
        name: 'background-color',
        inherited: false,
        syntax: '<color> | transparent',
        keywords: [
          'transparent'
        ],
        examples: [
          '#f4f1ea',
          'transparent'
        ]
      },
      {
        name: 'background-image',
        inherited: false,
        syntax: 'none | <url>',
        keywords: [
          'none'
        ],
        examples: [
          'none',
          'url("verso.webp")',
          'url(art/plate.png)'
        ]
      },
      {
        name: 'background-repeat',
        inherited: false,
        syntax: 'repeat | no-repeat',
        keywords: [
          'repeat',
          'no-repeat'
        ],
        examples: [
          'repeat',
          'no-repeat'
        ]
      },
      {
        name: 'background-size',
        inherited: false,
        syntax: 'auto | cover | contain | [ <length> | <percentage> | auto ]{1,2}',
        keywords: [
          'auto',
          'cover',
          'contain'
        ],
        examples: [
          'auto',
          'cover',
          'contain',
          '120pt',
          '100% auto'
        ]
      },
      {
        name: 'background-position',
        inherited: false,
        syntax: '[ left | center | right | <length> | <percentage> ] [ top | center | bottom | <length> | <percentage> ]?',
        keywords: [
          'left',
          'center',
          'right',
          'top',
          'bottom'
        ],
        examples: [
          'center',
          'right bottom',
          '50% 50%',
          '12pt 18pt',
          'top left'
        ]
      },
      {
        name: 'column-count',
        inherited: false,
        syntax: 'auto | <integer>',
        keywords: [
          'auto'
        ],
        examples: [
          'auto',
          '2'
        ]
      },
      {
        name: 'column-width',
        inherited: false,
        syntax: 'auto | <length>',
        keywords: [
          'auto'
        ],
        examples: [
          'auto',
          '160pt'
        ]
      },
      {
        name: 'column-gap',
        inherited: false,
        syntax: 'normal | <length>',
        keywords: [
          'normal'
        ],
        examples: [
          'normal',
          '18pt'
        ]
      },
      {
        name: 'column-rule-width',
        inherited: false,
        syntax: '<length> | thin | medium | thick',
        keywords: [
          'thin',
          'medium',
          'thick'
        ],
        examples: [
          '0.5pt',
          'thin',
          'medium',
          'thick'
        ]
      },
      {
        name: 'column-rule-style',
        inherited: false,
        syntax: 'none | solid',
        keywords: [
          'none',
          'solid'
        ],
        examples: [
          'none',
          'solid'
        ]
      },
      {
        name: 'align-content',
        inherited: false,
        syntax: 'start | center | end',
        keywords: [
          'start',
          'center',
          'end'
        ],
        examples: [
          'start',
          'center',
          'end'
        ]
      }
    ],
    margin_boxes: [
      {
        name: 'top-left-corner',
        paints: false
      },
      {
        name: 'top-left',
        paints: true
      },
      {
        name: 'top-center',
        paints: true
      },
      {
        name: 'top-right',
        paints: true
      },
      {
        name: 'top-right-corner',
        paints: false
      },
      {
        name: 'left-top',
        paints: false
      },
      {
        name: 'left-middle',
        paints: false
      },
      {
        name: 'left-bottom',
        paints: false
      },
      {
        name: 'right-top',
        paints: false
      },
      {
        name: 'right-middle',
        paints: false
      },
      {
        name: 'right-bottom',
        paints: false
      },
      {
        name: 'bottom-left-corner',
        paints: false
      },
      {
        name: 'bottom-left',
        paints: true
      },
      {
        name: 'bottom-center',
        paints: true
      },
      {
        name: 'bottom-right',
        paints: true
      },
      {
        name: 'bottom-right-corner',
        paints: false
      }
    ],
    margin_box_properties: [
      {
        name: 'content',
        inherited: false,
        syntax: 'none | <string> | counter(page) | counter(page, <counter-style>) | string(<name>)',
        keywords: [
          'none'
        ],
        examples: [
          'none',
          '"Chapter"',
          'counter(page)',
          'counter(page, lower-roman)',
          'string(chapter)'
        ]
      }
    ],
    sizes: [
      {
        name: 'a3',
        width: 841.8898,
        height: 1190.5511
      },
      {
        name: 'a4',
        width: 595.2756,
        height: 841.8898
      },
      {
        name: 'a5',
        width: 419.52756,
        height: 595.2756
      },
      {
        name: 'b4',
        width: 708.66144,
        height: 1000.62994
      },
      {
        name: 'b5',
        width: 498.89764,
        height: 708.66144
      },
      {
        name: 'letter',
        width: 612.0,
        height: 792.0
      },
      {
        name: 'legal',
        width: 612.0,
        height: 1008.0
      },
      {
        name: 'ledger',
        width: 792.0,
        height: 1224.0
      }
    ],
    counter_styles: [
      'decimal',
      'lower-roman',
      'upper-roman',
      'lower-alpha',
      'upper-alpha'
    ]
  },
  font_face: {
    descriptors: [
      {
        name: 'font-family',
        syntax: '<family-name>',
        keywords: [],
        examples: [
          '"Author Serif"',
          'Author Serif'
        ]
      },
      {
        name: 'font-style',
        syntax: 'normal | italic | oblique',
        keywords: [
          'normal',
          'italic',
          'oblique'
        ],
        examples: [
          'italic'
        ]
      },
      {
        name: 'font-weight',
        syntax: 'normal | bold | <number [1,1000]>',
        keywords: [
          'normal',
          'bold'
        ],
        examples: [
          'bold',
          '600'
        ]
      },
      {
        name: 'src',
        syntax: '[ <url> format(<string>)? | local(<string>) ]#',
        keywords: [],
        examples: [
          'url(fonts/serif.otf)',
          'url("fonts/serif.woff2") format("woff2")',
          'local("Author Serif"), url(fonts/serif.otf)'
        ]
      }
    ]
  },
  units: [
    'pt',
    'px',
    'pc',
    'in',
    'cm',
    'mm',
    'q',
    'em',
    'rem'
  ],
  color_names: [
    'aliceblue',
    'antiquewhite',
    'aqua',
    'aquamarine',
    'azure',
    'beige',
    'bisque',
    'black',
    'blanchedalmond',
    'blue',
    'blueviolet',
    'brown',
    'burlywood',
    'cadetblue',
    'chartreuse',
    'chocolate',
    'coral',
    'cornflowerblue',
    'cornsilk',
    'crimson',
    'cyan',
    'darkblue',
    'darkcyan',
    'darkgoldenrod',
    'darkgray',
    'darkgreen',
    'darkgrey',
    'darkkhaki',
    'darkmagenta',
    'darkolivegreen',
    'darkorange',
    'darkorchid',
    'darkred',
    'darksalmon',
    'darkseagreen',
    'darkslateblue',
    'darkslategray',
    'darkslategrey',
    'darkturquoise',
    'darkviolet',
    'deeppink',
    'deepskyblue',
    'dimgray',
    'dimgrey',
    'dodgerblue',
    'firebrick',
    'floralwhite',
    'forestgreen',
    'fuchsia',
    'gainsboro',
    'ghostwhite',
    'gold',
    'goldenrod',
    'gray',
    'green',
    'greenyellow',
    'grey',
    'honeydew',
    'hotpink',
    'indianred',
    'indigo',
    'ivory',
    'khaki',
    'lavender',
    'lavenderblush',
    'lawngreen',
    'lemonchiffon',
    'lightblue',
    'lightcoral',
    'lightcyan',
    'lightgoldenrodyellow',
    'lightgray',
    'lightgreen',
    'lightgrey',
    'lightpink',
    'lightsalmon',
    'lightseagreen',
    'lightskyblue',
    'lightslategray',
    'lightslategrey',
    'lightsteelblue',
    'lightyellow',
    'lime',
    'limegreen',
    'linen',
    'magenta',
    'maroon',
    'mediumaquamarine',
    'mediumblue',
    'mediumorchid',
    'mediumpurple',
    'mediumseagreen',
    'mediumslateblue',
    'mediumspringgreen',
    'mediumturquoise',
    'mediumvioletred',
    'midnightblue',
    'mintcream',
    'mistyrose',
    'moccasin',
    'navajowhite',
    'navy',
    'oldlace',
    'olive',
    'olivedrab',
    'orange',
    'orangered',
    'orchid',
    'palegoldenrod',
    'palegreen',
    'paleturquoise',
    'palevioletred',
    'papayawhip',
    'peachpuff',
    'peru',
    'pink',
    'plum',
    'powderblue',
    'purple',
    'rebeccapurple',
    'red',
    'rosybrown',
    'royalblue',
    'saddlebrown',
    'salmon',
    'sandybrown',
    'seagreen',
    'seashell',
    'sienna',
    'silver',
    'skyblue',
    'slateblue',
    'slategray',
    'slategrey',
    'snow',
    'springgreen',
    'steelblue',
    'tan',
    'teal',
    'thistle',
    'tomato',
    'turquoise',
    'violet',
    'wheat',
    'white',
    'whitesmoke',
    'yellow',
    'yellowgreen'
  ]
};
