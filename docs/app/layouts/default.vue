<script setup lang="ts"></script>

<template>
  <div class="w-screen h-screen">
    <div class="bg-gradient pointer-events-none touch-none fixed inset-0"></div>
    <div class="w-full h-full inset-0">
      <div class="w-full h-full flex flex-col">
        <!-- Nav Bar-->
        <div
          class="w-full max-h-24 flex justify-between items-center py-8 px-16"
        >
          <div class="flex gap-12 items-center">
            <NuxtLink to="/">
              <NuxtImg class="xl:w-36 w-24" src="/images/legato.svg" />
            </NuxtLink>
            <div class="flex sm:gap-12 gap-6 font-light">
              <NuxtLink class="text-(--text-secondary)" to="/docs"
                >Docs</NuxtLink
              >
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
        <div class="w-full h-full overflow-y-auto px-6 py-3 md:px-16 md:py-6">
          <slot />
        </div>
      </div>
    </div>
  </div>
</template>
