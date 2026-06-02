<script lang="ts">
    import { onMount } from "svelte";
    import gsap from "gsap";

    onMount(() => {
      // Whole-robot float
      gsap.to("#robot-full", {
        y: -8,
        duration: 2.2,
        ease: "sine.inOut",
        yoyo: true,
        repeat: -1,
      });

      // Arm swing
      gsap.to("#arm-left", {
        rotation: 12,
        svgOrigin: "25 68",
        duration: 1.1,
        ease: "sine.inOut",
        yoyo: true,
        repeat: -1,
      });
      gsap.to("#arm-right", {
        rotation: -12,
        svgOrigin: "95 68",
        duration: 1.1,
        ease: "sine.inOut",
        yoyo: true,
        repeat: -1,
        delay: 0.55,
      });

      // Screen scanline pulse
      gsap.to("#screen-scanline", {
        opacity: 0.04,
        duration: 0.08,
        yoyo: true,
        repeat: -1,
        repeatDelay: 2.5,
      });

      // Screen bars flicker
      gsap.to("#screen-bars", {
        opacity: 0.6,
        duration: 0.05,
        yoyo: true,
        repeat: -1,
        repeatDelay: 4,
      });
    });

    export function stop() {
      gsap.killTweensOf("#robot-full, #arm-left, #arm-right, #screen-scanline, #screen-bars");
    }

    export function startBlinking() {
      gsap.to("#screen-bars", {
        opacity: 0,
        duration: 0.1,
        ease: "power2.in",
        yoyo: true,
        repeat: -1,
        repeatDelay: 0.5,
      });
    }
</script>

<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 220" style="display:block; width:100%; height:100%">
  <defs>
    <filter id="shadow" x="-25%" y="-25%" width="150%" height="150%">
      <feDropShadow dx="1" dy="2" stdDeviation="1.8" flood-color="#00000077"/>
    </filter>
    <filter id="glow">
      <feGaussianBlur stdDeviation="1.2" result="blur"/>
      <feMerge><feMergeNode in="blur"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>

    <!-- Head case: weathered cream -->
    <linearGradient id="head-case-grad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#d4d0c4"/>
      <stop offset="55%" stop-color="#aaa89c"/>
      <stop offset="100%" stop-color="#6a6860"/>
    </linearGradient>

    <!-- Vertical pipe: cylindrical lit from left -->
    <linearGradient id="pipe-cyl" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#98b8c0"/>
      <stop offset="15%" stop-color="#dedad0"/>
      <stop offset="58%" stop-color="#6a9aaa"/>
      <stop offset="100%" stop-color="#1e3848"/>
    </linearGradient>

    <!-- Joint ball -->
    <radialGradient id="joint-grad" cx="32%" cy="28%" r="65%">
      <stop offset="0%" stop-color="#e0ddd0"/>
      <stop offset="50%" stop-color="#8ab0b8"/>
      <stop offset="100%" stop-color="#1e3848"/>
    </radialGradient>

    <!-- Torso / hips -->
    <linearGradient id="torso-grad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#c4c0b4"/>
      <stop offset="45%" stop-color="#8aaab2"/>
      <stop offset="100%" stop-color="#2a5060"/>
    </linearGradient>

    <!-- Screen -->
    <linearGradient id="screen-grad" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#002800"/>
      <stop offset="100%" stop-color="#000e00"/>
    </linearGradient>

    <!-- Foot -->
    <linearGradient id="foot-grad" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#b0b0a4"/>
      <stop offset="100%" stop-color="#505048"/>
    </linearGradient>
  </defs>

  <!-- Ground shadow -->
  <ellipse cx="60" cy="215" rx="36" ry="5" fill="#00000030"/>

  <g id="robot-full">

    <!-- ══ LEFT LEG ══ -->
    <!-- Hip joint -->
    <circle cx="44" cy="130" r="8" fill="url(#joint-grad)" filter="url(#shadow)"/>
    <circle cx="42" cy="128" r="3.5" fill="white" opacity="0.3"/>
    <!-- Upper leg pipe -->
    <rect x="36" y="130" width="16" height="42" rx="8" fill="url(#pipe-cyl)" filter="url(#shadow)"/>
    <rect x="37" y="131" width="5" height="38" rx="2.5" fill="white" opacity="0.2"/>
    <ellipse cx="44" cy="148" rx="10" ry="3.5" fill="#3a98a8" opacity="0.38"/>
    <!-- Knee joint -->
    <circle cx="44" cy="174" r="9" fill="url(#joint-grad)" filter="url(#shadow)"/>
    <circle cx="42" cy="172" r="3.8" fill="white" opacity="0.28"/>
    <!-- Lower leg pipe -->
    <rect x="36" y="174" width="16" height="34" rx="8" fill="url(#pipe-cyl)" filter="url(#shadow)"/>
    <rect x="37" y="175" width="5" height="30" rx="2.5" fill="white" opacity="0.18"/>
    <!-- Foot -->
    <rect x="28" y="203" width="28" height="11" rx="4" fill="url(#foot-grad)" filter="url(#shadow)"/>
    <rect x="29" y="204" width="14" height="4" rx="2" fill="white" opacity="0.2"/>

    <!-- ══ RIGHT LEG ══ -->
    <!-- Hip joint -->
    <circle cx="76" cy="130" r="8" fill="url(#joint-grad)" filter="url(#shadow)"/>
    <circle cx="74" cy="128" r="3.5" fill="white" opacity="0.3"/>
    <!-- Upper leg pipe -->
    <rect x="68" y="130" width="16" height="42" rx="8" fill="url(#pipe-cyl)" filter="url(#shadow)"/>
    <rect x="69" y="131" width="5" height="38" rx="2.5" fill="white" opacity="0.18"/>
    <ellipse cx="76" cy="148" rx="10" ry="3.5" fill="#3a98a8" opacity="0.32"/>
    <!-- Knee joint -->
    <circle cx="76" cy="174" r="9" fill="url(#joint-grad)" filter="url(#shadow)"/>
    <circle cx="74" cy="172" r="3.8" fill="white" opacity="0.25"/>
    <!-- Lower leg pipe -->
    <rect x="68" y="174" width="16" height="34" rx="8" fill="url(#pipe-cyl)" filter="url(#shadow)"/>
    <rect x="69" y="175" width="5" height="30" rx="2.5" fill="white" opacity="0.16"/>
    <!-- Foot -->
    <rect x="64" y="203" width="28" height="11" rx="4" fill="url(#foot-grad)" filter="url(#shadow)"/>
    <rect x="65" y="204" width="14" height="4" rx="2" fill="white" opacity="0.18"/>

    <!-- ══ TORSO ══ -->
    <rect x="28" y="62" width="64" height="54" rx="4" fill="url(#torso-grad)" filter="url(#shadow)"/>
    <!-- Segment ring lines -->
    <rect x="28" y="70" width="64" height="1.5" rx="0.75" fill="#1a3040" opacity="0.5"/>
    <rect x="28" y="78" width="64" height="1.5" rx="0.75" fill="#1a3040" opacity="0.5"/>
    <rect x="28" y="86" width="64" height="1.5" rx="0.75" fill="#1a3040" opacity="0.5"/>
    <rect x="28" y="94" width="64" height="1.5" rx="0.75" fill="#1a3040" opacity="0.4"/>
    <rect x="28" y="102" width="64" height="1.5" rx="0.75" fill="#1a3040" opacity="0.4"/>
    <!-- Teal paint patches -->
    <ellipse cx="48" cy="82" rx="13" ry="7" fill="#3a98a8" opacity="0.42"/>
    <ellipse cx="74" cy="96" rx="9" ry="5" fill="#2a8090" opacity="0.35"/>
    <!-- Chest highlight -->
    <rect x="30" y="63" width="28" height="5" rx="2" fill="white" opacity="0.2"/>
    <!-- Right edge shadow -->
    <rect x="84" y="64" width="6" height="50" rx="3" fill="#000" opacity="0.1"/>

    <!-- ══ HIPS / PELVIS ══ -->
    <rect x="30" y="116" width="60" height="16" rx="4" fill="url(#torso-grad)" filter="url(#shadow)"/>
    <rect x="30" y="119" width="60" height="1.2" rx="0.6" fill="#1a3040" opacity="0.5"/>
    <rect x="30" y="123" width="60" height="1.2" rx="0.6" fill="#1a3040" opacity="0.4"/>
    <rect x="32" y="117" width="22" height="3" rx="1.5" fill="white" opacity="0.18"/>

    <!-- ══ LEFT ARM ══ -->
    <g id="arm-left">
      <!-- Shoulder joint -->
      <circle cx="25" cy="68" r="9" fill="url(#joint-grad)" filter="url(#shadow)"/>
      <circle cx="23" cy="66" r="3.8" fill="white" opacity="0.35"/>
      <!-- Upper arm pipe -->
      <rect x="18" y="68" width="14" height="40" rx="7" fill="url(#pipe-cyl)" filter="url(#shadow)"/>
      <rect x="19" y="70" width="4.5" height="36" rx="2" fill="white" opacity="0.22"/>
      <ellipse cx="25" cy="84" rx="8" ry="3" fill="#3a98a8" opacity="0.38"/>
      <line x1="20" y1="72" x2="23" y2="105" stroke="#c0bdb0" stroke-width="0.6" opacity="0.35"/>
      <!-- Elbow joint -->
      <circle cx="25" cy="110" r="8" fill="url(#joint-grad)" filter="url(#shadow)"/>
      <circle cx="23" cy="108" r="3.2" fill="white" opacity="0.3"/>
      <!-- Forearm pipe -->
      <rect x="18" y="110" width="14" height="34" rx="7" fill="url(#pipe-cyl)" filter="url(#shadow)"/>
      <rect x="19" y="112" width="4.5" height="30" rx="2" fill="white" opacity="0.18"/>
      <ellipse cx="25" cy="124" rx="7" ry="2.8" fill="#2a8090" opacity="0.32"/>
      <!-- Hand -->
      <ellipse cx="25" cy="147" rx="9" ry="8" fill="url(#joint-grad)" filter="url(#shadow)"/>
      <ellipse cx="23" cy="144" rx="4.5" ry="3.5" fill="white" opacity="0.28"/>
    </g>

    <!-- ══ RIGHT ARM ══ -->
    <g id="arm-right">
      <!-- Shoulder joint -->
      <circle cx="95" cy="68" r="9" fill="url(#joint-grad)" filter="url(#shadow)"/>
      <circle cx="93" cy="66" r="3.8" fill="white" opacity="0.3"/>
      <!-- Upper arm pipe -->
      <rect x="88" y="68" width="14" height="40" rx="7" fill="url(#pipe-cyl)" filter="url(#shadow)"/>
      <rect x="89" y="70" width="4.5" height="36" rx="2" fill="white" opacity="0.18"/>
      <ellipse cx="95" cy="84" rx="8" ry="3" fill="#3a98a8" opacity="0.35"/>
      <line x1="90" y1="72" x2="93" y2="105" stroke="#c0bdb0" stroke-width="0.6" opacity="0.32"/>
      <!-- Elbow joint -->
      <circle cx="95" cy="110" r="8" fill="url(#joint-grad)" filter="url(#shadow)"/>
      <circle cx="93" cy="108" r="3.2" fill="white" opacity="0.28"/>
      <!-- Forearm pipe -->
      <rect x="88" y="110" width="14" height="34" rx="7" fill="url(#pipe-cyl)" filter="url(#shadow)"/>
      <rect x="89" y="112" width="4.5" height="30" rx="2" fill="white" opacity="0.16"/>
      <ellipse cx="95" cy="124" rx="7" ry="2.8" fill="#2a8090" opacity="0.3"/>
      <!-- Hand -->
      <ellipse cx="95" cy="147" rx="9" ry="8" fill="url(#joint-grad)" filter="url(#shadow)"/>
      <ellipse cx="93" cy="144" rx="4.5" ry="3.5" fill="white" opacity="0.25"/>
    </g>

    <!-- ══ NECK ══ -->
    <rect id="robot-neck" x="53" y="52" width="14" height="12" rx="4" fill="url(#pipe-cyl)"/>
    <rect x="53" y="56" width="14" height="1.5" rx="0.75" fill="#1a3040" opacity="0.55"/>
    <rect x="53" y="52.5" width="14" height="2.5" rx="1.2" fill="white" opacity="0.22"/>

    <!-- ══ HEAD (monitor box) ══ -->
    <rect id="robot-head" x="35" y="8" width="50" height="45" rx="4" fill="url(#head-case-grad)" filter="url(#shadow)"/>
    <!-- Case top highlight -->
    <rect x="37" y="9" width="32" height="6" rx="3" fill="white" opacity="0.28"/>
    <!-- Right-side depth shadow -->
    <rect x="78" y="10" width="5" height="41" rx="2" fill="#000" opacity="0.13"/>
    <!-- Teal paint chips on case -->
    <ellipse cx="72" cy="38" rx="7" ry="4.5" fill="#3a98a8" opacity="0.38"/>
    <ellipse cx="42" cy="45" rx="5" ry="3" fill="#2a8090" opacity="0.3"/>
    <!-- Corner bolts -->
    <circle cx="39" cy="12" r="2.2" fill="#585850"/>
    <circle cx="81" cy="12" r="2.2" fill="#585850"/>
    <circle cx="39" cy="49" r="2.2" fill="#585850"/>
    <circle cx="81" cy="49" r="2.2" fill="#585850"/>
    <circle cx="38.5" cy="11.5" r="0.9" fill="white" opacity="0.55"/>
    <circle cx="80.5" cy="11.5" r="0.9" fill="white" opacity="0.55"/>

    <!-- Screen bezel -->
    <rect x="39" y="16" width="42" height="26" rx="2.5" fill="#090c09"/>
    <!-- Screen surface -->
    <rect x="40" y="17" width="40" height="24" rx="1.5" fill="url(#screen-grad)"/>

    <!-- Green display bars -->
    <g id="screen-bars">
      <rect x="42" y="19" width="36" height="3.2" rx="0.8" fill="#00ff44" opacity="0.95"/>
      <rect x="42" y="23.5" width="26" height="2.6" rx="0.8" fill="#00ee44" opacity="0.85"/>
      <rect x="42" y="27.5" width="32" height="2.6" rx="0.8" fill="#00dd44" opacity="0.75"/>
      <rect x="42" y="31.5" width="18" height="2.6" rx="0.8" fill="#00cc44" opacity="0.65"/>
      <rect x="42" y="35.5" width="28" height="2.6" rx="0.8" fill="#00bb44" opacity="0.5"/>
    </g>
    <!-- Scanline overlay -->
    <rect id="screen-scanline" x="40" y="17" width="40" height="24" rx="1.5" fill="white" opacity="0.06"/>
    <!-- Screen glow edge -->
    <rect x="40" y="17" width="40" height="24" rx="1.5" fill="none" stroke="#00ff44" stroke-width="0.6" opacity="0.5" filter="url(#glow)"/>

    <!-- Vents at bottom of head -->
    <rect x="44" y="46" width="2.2" height="4" rx="1" fill="#484840"/>
    <rect x="48" y="46" width="2.2" height="4" rx="1" fill="#484840"/>
    <rect x="52" y="46" width="2.2" height="4" rx="1" fill="#484840"/>
    <rect x="56" y="46" width="2.2" height="4" rx="1" fill="#484840"/>
    <rect x="60" y="46" width="2.2" height="4" rx="1" fill="#484840"/>
    <rect x="64" y="46" width="2.2" height="4" rx="1" fill="#484840"/>
    <rect x="68" y="46" width="2.2" height="4" rx="1" fill="#484840"/>
    <rect x="72" y="46" width="2.2" height="4" rx="1" fill="#484840"/>

  </g>
</svg>
