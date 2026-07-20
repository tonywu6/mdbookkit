<p><details>
  <summary>Example usage</summary>

<figure>

```shell
MDBOOK_LOG=info mdbook build
```

<figcaption>
  Enable logging at level <code>info</code> or above (which is the default level)
</figcaption>

</figure>

<figure>

```shell
MDBOOK_LOG=debug mdbook build
```

<figcaption>
  Enable logging at level <code>debug</code> or above (more verbose logging)
</figcaption>

</figure>

<figure>

```shell
MDBOOK_LOG=info,{{ preprocessor|snake_case }}=trace mdbook build
```

<figcaption>
  Enable logging at level <code>trace</code> or above for <code>{{ preprocessor }}</code>
  (extremely verbose logging), and at level <code>info</code> or above for everything else,
  such as <code>mdbook</code> itself. <br> Note that you should use <code>snake_case</code>
  and not <code>kebab-case</code> when mentioning the program.
</figcaption>

</figure>

</details></p>
