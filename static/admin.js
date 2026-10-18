// Small helpers for every admin page. The pages work without it, except copy, local times, expiry and search.
(function () {
  // A Date as a datetime-local value in the browser's zone.
  function localValue(d) {
    return new Date(d.getTime() - d.getTimezoneOffset() * 60000).toISOString().slice(0, 16);
  }

  // Times are rendered in UTC; show them in the browser's zone.
  document.querySelectorAll('time[datetime]').forEach(function (el) {
    var d = new Date(el.getAttribute('datetime'));
    if (!isNaN(d)) el.textContent = d.toLocaleString(document.documentElement.lang, { dateStyle: 'medium', timeStyle: 'short' });
  });
  document.querySelectorAll('input[data-initial]').forEach(function (el) {
    el.value = el.dataset.initialLocal = localValue(new Date(el.dataset.initial));
  });

  document.addEventListener('submit', function (e) {
    var form = e.target;
    var msg = form.getAttribute('data-confirm');
    if (msg && !window.confirm(msg)) return e.preventDefault();
    // datetime-local has no zone; send the instant the owner meant, in UTC. Untouched, the target keeps `keep`.
    form.querySelectorAll('input[data-local-for]').forEach(function (local) {
      if (local.value === (local.dataset.initialLocal || '')) return;
      form.elements[local.getAttribute('data-local-for')].value = local.value ? new Date(local.value).toISOString() : '';
    });
  });

  document.addEventListener('click', function (e) {
    var chip = e.target.closest('[data-days]');
    if (chip) {
      var input = chip.closest('.expiry').querySelector('input');
      var days = Number(chip.dataset.days);
      input.value = days ? localValue(new Date(Date.now() + days * 86400000)) : '';
      input.dispatchEvent(new Event('input', { bubbles: true }));
      return;
    }
    var reveal = e.target.closest('[data-reveal]');
    if (reveal) {
      var field = document.getElementById(reveal.dataset.reveal);
      var shown = field.type === 'password';
      field.type = shown ? 'text' : 'password';
      reveal.setAttribute('aria-pressed', String(shown));
      return;
    }
    var btn = e.target.closest('[data-copy]');
    if (!btn) return;
    var value = document.getElementById(btn.getAttribute('data-copy')).value;
    navigator.clipboard.writeText(value).then(function () {
      // Only the text changes, so the button keeps its icon.
      var label = btn.querySelector('.label') || btn;
      var old = label.textContent;
      label.textContent = '✓';
      btn.classList.add('copied');
      setTimeout(function () { label.textContent = old; btn.classList.remove('copied'); }, 1200);
    });
  });

  // <x data-disable-if="name=value"> disables its controls while the form control `name` has that value.
  function sync() {
    document.querySelectorAll('[data-disable-if]').forEach(function (el) {
      var kv = el.getAttribute('data-disable-if').split('=');
      var control = el.closest('form').elements[kv[0]];
      var off = !!control && control.value === kv[1];
      el.classList.toggle('disabled', off);
      el.querySelectorAll('input, button').forEach(function (c) { c.disabled = off; });
    });
  }
  document.addEventListener('change', sync);
  sync();

  // Share list: match the search against everything a row shows, and the creation date against the range.
  var search = document.getElementById('search');
  if (!search) return;
  var from = document.getElementById('from');
  var to = document.getElementById('to');
  // Wrap each case-insensitive hit of `word` in the text under `root` in <mark>.
  function mark(root, word) {
    var walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    var nodes = [];
    while (walker.nextNode()) nodes.push(walker.currentNode);
    nodes.forEach(function (node) {
      var i;
      while (node && (i = node.data.toLowerCase().indexOf(word)) >= 0) {
        var hit = node.splitText(i);
        node = hit.splitText(word.length);
        var m = document.createElement('mark');
        hit.replaceWith(m);
        m.appendChild(hit);
      }
    });
  }
  function filter() {
    var words = search.value.toLowerCase().split(/\s+/).filter(Boolean);
    var lo = from.value ? new Date(from.value + 'T00:00') : null;
    var hi = to.value ? new Date(to.value + 'T23:59:59.999') : null;
    var shown = 0;
    document.querySelectorAll('li.share').forEach(function (li) {
      // Only the row's own fields count, not its button labels.
      var main = li.querySelector('.share-main');
      main.querySelectorAll('mark').forEach(function (m) { m.replaceWith(m.textContent); });
      main.normalize();
      var text = main.textContent.toLowerCase();
      var created = new Date(li.dataset.created);
      var ok = words.every(function (w) { return text.indexOf(w) >= 0; }) && !(lo && created < lo) && !(hi && created > hi);
      li.hidden = !ok;
      if (ok) words.forEach(function (w) { mark(main, w); });
      if (ok) shown++;
    });
    var none = document.getElementById('no-match');
    if (none) none.hidden = shown > 0;
  }
  [search, from, to].forEach(function (el) { el.addEventListener('input', filter); });
})();
