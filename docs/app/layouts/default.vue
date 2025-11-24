<script setup lang="ts"></script>

<template>
  <div class="bg-gradient"></div>
  <div class="w-screen h-screen bg-(--bg)">
    <div class="w-full flex justify-between p-6">
      <h2>Legato</h2>
      <NuxtLink to="/docs">Docs</NuxtLink>
    </div>
    <slot />
  </div>
</template>
