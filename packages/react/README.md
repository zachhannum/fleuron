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

The manuscript and the stylesheet are props, so an edit is a re-render
and the engine is handed the one input that changed. A render the
reader has already typed past paints nothing.

`onMount` hands back the preview itself, for the page count, the
warnings and the PDF export.

There is no engine logic in here. This package hands `fleuron`'s
`Preview` an element and passes props along to it. React stays out of
the binding package, so a host that does not use React downloads none
of it.

MIT or Apache-2.0.
