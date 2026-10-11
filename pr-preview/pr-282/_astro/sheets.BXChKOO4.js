var e=`@page {
  size: 5.5in 8.5in;
  margin: 0.7in 0.6in 0.8in 0.7in;

  @bottom-center {
    content: counter(page);
    font-size: 9pt;
  }
}

book {
  font-size: 11pt;
  line-height: 1.4;
  text-align: justify;
  hyphens: auto;
}

section { break-before: recto; }

p {
  margin: 0;
  text-indent: 1.15em;
  orphans: 2;
  widows: 2;
}
`,t=e.replace(`line-height: 1.4;`,`line-height: 1.5;`),n=`## A Chapter

It is a truth universally acknowledged, that a single man in
possession of a good fortune, must be in want of a wife.

However little known the feelings or views of such a man may be on his
first entering a neighbourhood, this truth is so well fixed in the
minds of the surrounding families, that he is considered the rightful
property of some one or other of their daughters.
`,r=`@page {
  size: 5.5in 8.5in;
  margin: 0.7in;
}

p {
  text-indent: 1.2em;
  color: crimson;
  border-radius: 3pt;
}

blockquote {
  display: flex;
}
`;export{n as i,r as n,t as r,e as t};