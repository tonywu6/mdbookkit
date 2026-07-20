<a href="/README.md" download>
  Click here to download the current version of the file
</a>

<figure>
  <img src="../static/image.jpg">
</figure>

<link rel="stylesheet" href="/docs/web/main.css" />

<p><object
  data="../static/image.jpg"
  type="image/jpeg">
  <em>Your browser does not <a href="/docs/">support</a> this type of content!</em>
</object></p>

<p>
  Special case when processing `src` attributes pointing to book pages:
  The program should convert it to end in `.html` because mdBook won't
  process elements other than anchors.
  <iframe src="./web-links.md"></iframe>
</p>

<p>
  This one too:
  <iframe src="https://book.example.org/web-links"></iframe>
</p>
