<p><details>
  <summary>Example usage</summary>

<figure>

<figcaption>
  Most platforms will configure the <code>CI</code> variable by default. These examples
  are for illustrative purposes, or if you would like to test the preprocessor's behavior locally.
</figcaption>

```shell
CI=1 mdbook build
```

```shell
CI=true mdbook build
```

```shell
CI=false mdbook build
```

<figcaption>
  The preprocessor runs in CI mode as long as the variable has any value, even <code>false</code>.
</figcaption>

```shell
CI= mdbook build
```

<figcaption>
  Setting <code>CI</code> to an empty value disables CI mode.
</figcaption>

</figure>

</details></p>

The preprocessor's default behaviors changes when it detects it is running in a
continuous integration (CI) environment via the `CI` environment variable.

See the [continuous integration guide](../how-to/continuous-integration.md) for detailed
information on using the preprocessor in CI.

You usually don't need to set this manually, since most platforms will set `CI=true` by
default.
