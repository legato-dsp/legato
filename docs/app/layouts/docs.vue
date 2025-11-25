<script lang="ts" setup>
const { data: docs } = await useAsyncData(() => {
  return queryCollection("docs").all();
});
</script>

<template>
  <div class="w-full h-full">
    <NuxtLayout name="default">
      <div class="w-full h-full flex gap-16">
        <div class="flex flex-col gap-3">
          <p v-for="doc in docs" :key="doc.id">
            <nuxt-link :to="doc.path">
              {{ doc.title }}
            </nuxt-link>
          </p>
        </div>
        <div class="docs-content">
          <slot />
        </div>
      </div>
    </NuxtLayout>
  </div>
</template>
