<script lang="ts" setup>
const { data: docs } = await useAsyncData(() => {
  return queryCollection("docs").all();
});
</script>

<template>
  <div class="w-full h-full">
    <NuxtLayout name="default">
      <div class="w-full h-full grid grid-cols-[192px_1fr_128px] gap-16">
        <div class="flex flex-col gap-3">
          <p v-for="doc in docs" :key="doc.id">
            <nuxt-link :to="doc.path">
              {{ doc.title }}
            </nuxt-link>
          </p>
        </div>
        <div class="flex flex-col justify-center">
          <div>
            <slot />
          </div>
        </div>
      </div>
    </NuxtLayout>
  </div>
</template>
