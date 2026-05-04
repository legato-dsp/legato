<script setup lang="ts">
const links = [
  { title: "Docs", to: "/docs" },
  { title: "Contributing", to: "/contributing" },
  { title: "About", to: "/about" },
];
</script>

<template>
  <div class="bg-gradient pointer-events-none touch-none fixed inset-0"></div>
  <div class="w-full h-full inset-0">
    <div class="w-full h-full flex flex-col">
      <!-- Nav Bar-->
      <div
        class="w-full max-h-24 flex justify-between items-center py-3 pt-4 sm:pt-3 md:py-8 px-3 sm:px-8 md:px-16"
      >
        <div class="flex gap-6 md:gap-12 items-center">
          <NuxtLink class="shrink-0" to="/">
            <NuxtImg
              class="md:w-36 sm:w-32 w-24 shrink-0 sm:mr-3"
              src="/images/legato.svg"
            />
          </NuxtLink>
          <NuxtLink
            v-for="item in links"
            :key="item.to"
            :to="item.to"
            activeClass="!opacity-100"
            class="transition-opacity opacity-60 text-sm sm:text-base"
          >
            {{ item.title }}
          </NuxtLink>
        </div>
        <a href="https://github.com/legato-dsp">
          <img class="w-6 sm:w-8" src="/images/github-mark-white.svg" />
        </a>
      </div>
      <!-- Content -->
      <div class="w-full h-full overflow-y-auto px-6 py-3 md:px-16 md:py-6">
        <slot></slot>
      </div>
    </div>
  </div>
</template>
