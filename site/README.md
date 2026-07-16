# barca-docs

The Barca documentation site — [Astro](https://astro.build) +
[Starlight](https://starlight.astro.build), source content in
`src/content/docs/`.

## Develop

```bash
npm install
npm run dev       # http://localhost:4321
```

## Build

```bash
npm run build     # outputs static site to dist/
npm run preview   # serve the built output locally
```

`npm run build` produces a fully static site — no server runtime or adapter
is required.

## Deploying to Cloudflare Pages

Create a Cloudflare Pages project pointed at this repository with:

| Setting | Value |
|---|---|
| Root directory | `site` |
| Build command | `npm run build` |
| Build output directory | `dist` |
| Framework preset | Astro |

No environment variables or adapters are needed since the output is static.
Attach a custom domain (e.g. `barca.sh`) from the Cloudflare Pages project
settings once the first deploy succeeds.
