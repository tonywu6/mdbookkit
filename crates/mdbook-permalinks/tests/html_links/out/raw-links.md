<a href="https://git.example.org/raw/[GIT_REVISION]/README.md" download>
  Click here to download the current version of the file
</a>

<figure>
  <img src="https://git.example.org/raw/[GIT_REVISION]/crates/mdbook-permalinks/tests/file_links/static/Minato_City,_Tokyo,_Japan.jpg">
</figure>

<link rel="stylesheet" href="https://git.example.org/raw/[GIT_REVISION]/docs/web/main.css" />

<p><object data="https://git.example.org/raw/[GIT_REVISION]/crates/mdbook-permalinks/tests/file_links/static/Minato_City,_Tokyo,_Japan.jpg" type="image/jpeg">
  <em>Your browser does not <a href="https://git.example.org/tree/[GIT_REVISION]/docs/">support</a> this type of content!</em>
</object></p>

<p>
  Special case when processing `src` attributes pointing to book pages:
  The program should convert it to end in `.html` because mdBook won't
  process elements other than anchors.
  <iframe src="web-links.html"></iframe>
</p>

<p>
  This one too:
  <iframe src="web-links.html"></iframe>
</p>
