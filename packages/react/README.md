# fleuron-react

The [fleuron](https://fleuron.typeworks.dev/) preview, as a
React component.

```sh
npm install fleuron-react fleuron react
```

```jsx
import { Preview } from 'fleuron-react';

<Preview markdown={markdown} css={css} page={page} zoom={1.5} />;
```

The manuscript and the stylesheet are props, so a keystroke is a
re-render and the engine is handed the one input that changed. A
render the reader has already typed past paints nothing.

A click on a link follows it. A link to a place in the book turns to
the page of that place, and a link to a url opens the url in a new
window. The `onLink` prop takes the link and the click event before
the preview follows the link. If it returns `false`, the page does not
turn. If there is an `onLink`, the preview opens no url, and the host
decides what to do with it. A new function on a re-render replaces the
old one.

```jsx
<Preview
  markdown={markdown}
  onLink={(link) => {
    if (link.to.kind === 'uri') openTab(link.to.url);
  }}
/>
```

`onMount` hands back the preview itself, for the page count, the
warnings and the PDF export.

There is no engine logic in here. This package hands `fleuron`'s
`Preview` an element and passes props along to it. React stays out of
the binding package, and deleting this one leaves a preview a plain
page can still mount.

MIT or Apache-2.0.
