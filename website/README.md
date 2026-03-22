# pftui.com Marketing Website

This directory contains the marketing website for pftui, deployed to GitHub Pages.

## 🌐 Live Site

- **Production:** https://pftui.com (custom domain)
- **GitHub Pages:** https://skylarsimoncelli.github.io/pftui/

## 🏗️ Architecture

Pure HTML/CSS/JS with no frameworks and no build step. Optimized for speed and simplicity.

### Files

- `index.html`: Main page structure
- `style.css`: Dark theme styling (GitHub dark inspired)
- `script.js`: Interactivity (terminal animation, copy buttons, scroll effects)
- `favicon.svg`: Terminal $ icon
- `CNAME`: Custom domain configuration

## 🎨 Design

- **Theme:** Dark (#0d1117 background) with green/cyan accents
- **Fonts:** Inter (body), JetBrains Mono (code)
- **Layout:** Single-page, fully responsive (375px to 4K)
- **Animations:** Terminal typing effect, fade-in on scroll, smooth transitions
- **Performance:** <1s load, no external dependencies except Google Fonts

## 📦 Deployment

Automated via GitHub Actions (`.github/workflows/website.yml`):

1. Push to `master` branch (changes in `website/**`)
2. Workflow builds and deploys to GitHub Pages
3. Site live at both GitHub Pages URL and custom domain

## 🔧 Local Development

```bash
# Serve locally (Python)
cd website
python3 -m http.server 8000

# Or use any static server
npx serve .
```

## 📝 Content Updates

Edit `index.html` directly for:
- Feature descriptions
- Installation commands
- Version numbers
- Links

The site will auto-deploy on push to master.

## 🎯 SEO & Social

- Title: "pftui: Your portfolio's command center"
- Description: Terminal-based portfolio intelligence dashboard
- Open Graph & Twitter cards configured
- Screenshot: https://github.com/user-attachments/assets/a1b6b11a-5893-4b91-9ac9-e14a9c64a66b

## 🚀 Custom Domain Setup

The CNAME file points to `pftui.com`. To activate:

1. Add DNS A records for pftui.com pointing to GitHub Pages IPs:
   - 185.199.108.153
   - 185.199.109.153
   - 185.199.110.153
   - 185.199.111.153
2. Wait for DNS propagation (up to 24 hours)
3. GitHub will automatically detect and verify the domain
4. HTTPS will be auto-provisioned via Let's Encrypt

## 📊 Performance

- First Contentful Paint: <0.5s
- Time to Interactive: <1s
- Total page weight: ~40KB (HTML+CSS+JS)
- Fonts: ~200KB (Google Fonts, cached)
- Screenshot: ~800KB (served from GitHub, lazy loaded)

## 🎨 Design Inspiration

- **Warp terminal**: Modern terminal aesthetic
- **Bloomberg terminal**: Information density
- **Indie hacker landing pages**: Personality without corporate stiffness

---

Built with care. MIT licensed.
