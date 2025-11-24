<script setup lang="ts"></script>

<template>
  <div class="w-screen h-screen">
    <div class="bg-gradient fixed inset-0"></div>
    <div class="w-full h-full inset-0 bg-(--bg)">
      <div class="w-full flex justify-between p-6">
        <NuxtImg class="xl:w-48 w-32" src="/images/legato.svg" />
        <NuxtLink to="/docs">Docs</NuxtLink>
      </div>
      <slot />
    </div>
  </div>
</template>
