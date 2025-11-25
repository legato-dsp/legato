<script setup lang="ts"></script>

<template>
  <div class="w-screen h-screen">
    <div class="bg-gradient pointer-events-none touch-none fixed inset-0"></div>
    <div class="w-full h-full inset-0 bg-(--bg)">
      <div class="w-full flex justify-between items-center py-8 px-16">
        <div class="flex gap-12 items-center">
          <NuxtImg class="xl:w-48 w-36" src="/images/legato.svg" />
          <div class="flex gap-6">
            <NuxtLink class="text-(--text-secondary)" to="/docs">Docs</NuxtLink>
            <NuxtLink class="text-(--text-secondary)" to="/contributing"
              >Contributing</NuxtLink
            >
            <NuxtLink class="text-(--text-secondary)" to="/about"
              >About</NuxtLink
            >
          </div>
        </div>
        <a href="https://github.com/legato-dsp/legato">
          <img class="w-8" src="/images/github-mark-white.svg" />
        </a>
      </div>
      <div class="px-16 py-6">
        <slot />
      </div>
    </div>
  </div>
</template>
