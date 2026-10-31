/**
 * Karkinos IDE — Landing Page Scripts
 * Interactive Showcase & Clipboard Utility
 */

const screenshots = [
  {
    src: 'assets/screenshots/cargo-pipeline.svg',
    alt: 'Karkinos Cargo Pipeline Runner with Check, Run, Clippy, and Test controls'
  },
  {
    src: 'assets/screenshots/code-actions.svg',
    alt: 'Karkinos LSP Code Actions (Ctrl+.) with Auto-Import and Match Arm assists'
  },
  {
    src: 'assets/screenshots/inlay-hints.svg',
    alt: 'Karkinos Inlay Hints displaying compiler inferred types and parameter names'
  },
  {
    src: 'assets/screenshots/dap-debugger.svg',
    alt: 'Karkinos DAP Native Debugger with Breakpoints, Call Stack, and Variables inspector'
  },
  {
    src: 'assets/screenshots/terminal-pty.svg',
    alt: 'Karkinos Interactive PTY Terminal Dock with ANSI colors, shell prompt, and segmented tabs'
  }
];

let currentIndex = 0;

/**
 * Switch the active showcase screenshot and highlight the corresponding tab
 * @param {number} index
 */
function switchScreenshot(index) {
  if (index < 0 || index >= screenshots.length) return;
  currentIndex = index;

  const tabs = document.querySelectorAll('.showcase-tab');
  tabs.forEach((tab, i) => {
    if (i === index) {
      tab.classList.add('active');
    } else {
      tab.classList.remove('active');
    }
  });

  const img = document.getElementById('showcase-img');
  if (img) {
    img.style.opacity = '0.3';
    setTimeout(() => {
      img.src = screenshots[index].src;
      img.alt = screenshots[index].alt;
      img.style.opacity = '1';
    }, 120);
  }
}

/**
 * Copy the cargo install command to the clipboard
 */
function copyCargoCommand() {
  const commandElem = document.getElementById('cargo-command');
  const copyTextElem = document.getElementById('copy-text');
  if (!commandElem || !copyTextElem) return;

  const textToCopy = commandElem.innerText || commandElem.textContent;

  if (navigator.clipboard && window.isSecureContext) {
    navigator.clipboard.writeText(textToCopy).then(() => {
      showCopySuccess(copyTextElem);
    }).catch(() => {
      fallbackCopy(textToCopy, copyTextElem);
    });
  } else {
    fallbackCopy(textToCopy, copyTextElem);
  }
}

/**
 * Fallback copy method for older browsers or non-secure contexts
 */
function fallbackCopy(text, targetElem) {
  const textArea = document.createElement('textarea');
  textArea.value = text;
  textArea.style.position = 'fixed';
  textArea.style.left = '-999999px';
  textArea.style.top = '-999999px';
  document.body.appendChild(textArea);
  textArea.focus();
  textArea.select();

  try {
    document.execCommand('copy');
    showCopySuccess(targetElem);
  } catch (err) {
    console.error('Failed to copy to clipboard', err);
  } finally {
    document.body.removeChild(textArea);
  }
}

/**
 * Visual feedback for copy button
 */
function showCopySuccess(elem) {
  const originalText = elem.textContent;
  elem.textContent = 'Copied! ✓';
  elem.style.color = '#3fb950';

  setTimeout(() => {
    elem.textContent = originalText;
    elem.style.color = '';
  }, 2000);
}

// Global exposure for inline onclick handlers
window.switchScreenshot = switchScreenshot;
window.copyCargoCommand = copyCargoCommand;
