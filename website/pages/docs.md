<aside>

[[toc]]

</aside>

# Getting Started

Lightning CSS can be used as a library from JavaScript or Rust, or from a standalone CLI. It can also be wrapped as a plugin in other build tools, and it is built into [Parcel](https://parceljs.org) out of the box.

## From Node

First, install Lightning CSS using a package manager such as npm or Yarn.

```shell
npm install --save-dev lightningcss
```

Once installed, import the module and call one of the Lightning CSS APIs. The `transform` function compiles a CSS stylesheet from a [Node Buffer](https://nodejs.org/api/buffer.html). This example minifies the input CSS, and outputs the compiled code and a source map.

```js
import { transform } from 'lightningcss';

let { code, map } = transform({
  filename: 'style.css',
  code: Buffer.from('.foo { color: red }'),
  minify: true,
  sourceMap: true
});
```

See [Transpilation](transpilation.html) for details about syntax lowering and vendor prefixing CSS for your browser targets, and the draft syntax support in Lightning CSS. You can also use the `bundle` API to process `@import` rules and inline them – see [Bundling](bundling.html) for details.

The [TypeScript definitions](https://github.com/parcel-bundler/lightningcss/blob/master/node/index.d.ts) also include documentation for all API options.

## From Rust

Lightning CSS can also be used as a Rust library to parse, transform, and minify CSS. See the Rust API docs on [docs.rs](https://docs.rs/lightningcss).

## With Parcel

[Parcel](https://parceljs.org) includes Lightning CSS as the default CSS transformer. You should also add a `browserslist` property to your `package.json`, which defines the target browsers that your CSS will be compiled for.

While Lightning CSS handles the most commonly used PostCSS plugins like `autoprefixer`, `postcss-preset-env`, and CSS modules, you may still need PostCSS for more custom plugins like TailwindCSS. If that's the case, your PostCSS config will be picked up automatically. You can remove the plugins listed above from your PostCSS config, and they'll be handled by Lightning CSS.

You can also configure Lightning CSS in the `package.json` in the root of your project. Currently, three options are supported: [drafts](transpilation.html#draft-syntax), which can be used to enable CSS nesting and custom media queries, [pseudoClasses](transpilation.html#pseudo-class-replacement), which allows replacing some pseudo classes like `:focus-visible` with normal classes that can be applied via JavaScript (e.g. polyfills), and [cssModules](css-modules.html), which enables CSS modules globally rather than only for files ending in `.module.css`, or accepts an options object.

```json
{
  "@parcel/transformer-css": {
    "cssModules": true,
    "drafts": {
      "nesting": true,
      "customMedia": true,
      "scope": true,
    },
    "pseudoClasses": {
      "focusVisible": "focus-ring"
    }
  }
}
```

See the [Parcel docs](https://parceljs.org/languages/css) for more details.

## From Deno or in browser

The `lightningcss-wasm` package can be used in Deno or directly in browsers. This uses a WebAssembly build of Lightning CSS. Use `TextEncoder` and `TextDecoder` convert code from a string to a typed array and back.

```js
import init, { transform } from 'https://unpkg.com/lightningcss-wasm?module';

await init();

let {code, map} = transform({
  filename: 'style.css',
  code: new TextEncoder().encode('.foo { color: red }'),
  minify: true,
});

console.log(new TextDecoder().decode(code));
```

Note that the `bundle` and visitor APIs are not currently available in the WASM build.

## With webpack

[css-minimizer-webpack-plugin](https://webpack.js.org/plugins/css-minimizer-webpack-plugin/) has built in support for Lightning CSS. To use it, first install Lightning CSS in your project with a package manager like npm or Yarn:

```shell
npm install --save-dev lightningcss css-minimizer-webpack-plugin browserslist
```

Next, configure `css-minifier-webpack-plugin` to use Lightning CSS as the minifier. You can provide options using the `minimizerOptions` object. See [Transpilation](transpilation.html) for details.

```js
// webpack.config.js
const CssMinimizerPlugin = require('css-minimizer-webpack-plugin');
const lightningcss = require('lightningcss');
const browserslist = require('browserslist');

module.exports = {
  optimization: {
    minimize: true,
    minimizer: [
      new CssMinimizerPlugin({
        minify: CssMinimizerPlugin.lightningCssMinify,
        minimizerOptions: {
          targets: lightningcss.browserslistToTargets(browserslist('>= 0.25%'))
        },
      }),
    ],
  },
};
```

## With Vite

[vite-plugin-lightningcss](https://github.com/lawrencecchen/vite-plugin-lightningcss) provides support for [transpilation](transpilation.html) using Lightning CSS in Vite.

First, install it into your project:

```shell
npm install --save-dev vite-plugin-lightningcss
```

Then, add it to your Vite config. You can pass options to the `lightningcss` plugin, including a `browserslist` config and other options documented in [Transpilation](transpilation.html).

```js
// vite.config.ts
import lightningcss from 'vite-plugin-lightningcss';

export default {
  plugins: [
    lightningcss({
      browserslist: '>= 0.25%',
    }),
  ],
};
```

Note that Vite uses PostCSS and esbuild internally for processing and minifying CSS even with this plugin, but it can still be a good alterntive to PostCSS plugins like autoprefixer and postcss-preset-env.

## From the CLI

Lightning CSS includes a standalone CLI that can be used to compile, minify, and bundle CSS files. It can be used when you only need to compile CSS, and don't need more advanced functionality from a larger build tool such as code splitting and support for other languages.

To use the CLI, install the `lightningcss-cli` package with an npm compatible package manager:

```shell
npm install --save-dev lightningcss-cli
```

Then, you can run the `lightningcss` command via `npx`, `yarn`, or by setting up a script in your package.json.

```json
{
  "scripts": {
    "build": "lightningcss --minify --bundle --targets '>= 0.25%' input.css -o output.css"
  }
}
```

To see all of the available options, use the `--help` argument:

```shell
npx lightningcss --help
```

## Error recovery

By default, Lightning CSS is strict, and will error when parsing an invalid rule or declaration. However, sometimes you may encounter a third party library that you can't easily modify, which unintentionally contains invalid syntax, or IE-specific hacks. In these cases, you can enable the `errorRecovery` option (or `--error-recovery` CLI flag). This will skip over invalid rules and declarations, omitting them in the output, and producing a warning instead of an error. You should also open an issue or PR to fix the issue in the library if possible.

## Source maps

Lightning CSS supports generating source maps when compiling, minifying, and bundling your source code to make debugging easier. Use the `sourceMap` option to enable it when using the API, or the `--sourcemap` CLI flag.

If the input CSS came from another compiler such as SASS or Less, you can also pass an input source map to Lightning CSS using the `inputSourceMap` API option. This will map compiled locations back to their location in the original source code.

Finally, the `projectRoot` option can be used to make file paths in source maps relative to a root directory. This makes build stable between machines.
