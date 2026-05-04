<script setup lang="ts">
useSeoMeta({
  title: "Legato",
  ogTitle: "Legato",
  ogDescription:
    "Legato is a framework for quickly building audio applications in Rust",
  description:
    "Legato is a framework for quickly building audio applications in Rust",
});

useHead({
  title: "Legato",
  meta: [
    {
      name: "Legato is a framework for quickly building audio applications in Rust",
    },
  ],
});
</script>

<template>
  <NuxtLayout name="default">
    <NuxtPage :key="$route.path" />
  </NuxtLayout>
</template>
