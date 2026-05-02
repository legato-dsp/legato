<script setup lang="ts">
const { data } = await useAsyncData(() => {
  return queryCollection("contributing").first();
});
</script>

<template>
  <div class="w-full h-full flex justify-center">
    <div class="max-w-200">
      <article v-if="data" class="prose">
        <h1>{{ data.title }}</h1>
        <ContentRenderer :value="data"> </ContentRenderer>
      </article>
    </div>
  </div>
</template>
