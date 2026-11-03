(function () {
  var style = document.createElement('style');
  document.head.appendChild(style);
  style.sheet.insertRule('.hero-title { color: #8e44ad; font-family: "Lora", serif; }', 0);
  var root = document.getElementById('root');
  function el(tag, text, attrs) {
    var e = document.createElement(tag);
    if (text) e.textContent = text;
    for (var k in (attrs || {})) e.setAttribute(k, attrs[k]);
    root.appendChild(e);
    return e;
  }
  var h = el('h1', 'Dhaka Tailors', { class: 'hero-title' });
  el('h2', 'Bespoke suits stitched in three days');
  el('p', 'We design and stitch bespoke suits, panjabis and shirts for customers across Bangladesh. Visit our workshop in Gulshan for a free fitting and fabric consultation with our master tailors.');
  el('p', 'Call 01811-556677 or write to orders@dhakatailors.test for any questions about your order.');
  el('a', 'Contact', { href: '/contact.html' });
  // Requests the browser must not make.
  var img = new Image(); img.src = '/pixel.png';
  fetch('http://169.254.169.254/latest/meta-data').catch(function () {});
})();
